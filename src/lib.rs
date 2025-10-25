use std::alloc::{Layout, alloc, dealloc};
use std::ffi::{c_int, c_void};
use std::fmt::{self, Display};
use std::mem::{self, offset_of};
use std::ptr;
use std::sync::{LazyLock, Mutex};

use nounwind::nounwind;

unsafe extern "C" {
    static FIELD_COUNT_MASK: c_int;
    static TAG_MASK: c_int;
    static max_alloc_size: u64;
}

const FIELD_SIZE: usize = mem::size_of::<*const c_void>();

/// The alignment of allocated objects.
const ALIGNMENT: usize = const {
    // why Ord::max no const T_T (rhetorical question)
    let obj_align = mem::align_of::<StellaObj>();

    if obj_align > 8 { obj_align } else { 8 }
};

/// A FFI-compatible definition of `stella_object`.
#[repr(C)]
struct StellaObj {
    header: c_int,
    fields: [ObjPtr; 0],
}

/// A FFI-compatible definition of `enum TAG`.
#[repr(C)]
#[derive(strum::FromRepr, strum::Display, Debug, Clone, Copy)]
#[strum(serialize_all = "kebab-case")]
enum StellaTag {
    Zero,
    Succ,
    False,
    True,
    Fn,
    Ref,
    Unit,
    Tuple,
    Inl,
    Inr,
    Empty,
    Cons,
}

/// An enumeration of possible kinds of stella object fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StellaFieldKind {
    /// The field holds a pointer to another stella object.
    Obj,

    /// The field holds an arbitrary pointer.
    Raw,

    /// The field is not supposed to be there at all.
    Invalid,
}

impl StellaTag {
    /// Classifies a field (with the given 0-based `idx`) of a stella object with this tag.
    fn field_kind(self, idx: usize) -> StellaFieldKind {
        match self {
            StellaTag::Zero => StellaFieldKind::Invalid,

            StellaTag::Succ if idx == 0 => StellaFieldKind::Obj,
            StellaTag::Succ => StellaFieldKind::Invalid,

            StellaTag::False => StellaFieldKind::Invalid,
            StellaTag::True => StellaFieldKind::Invalid,

            StellaTag::Fn if idx == 0 => StellaFieldKind::Raw,
            StellaTag::Fn => StellaFieldKind::Obj,

            StellaTag::Ref if idx == 0 => StellaFieldKind::Obj,
            StellaTag::Ref => StellaFieldKind::Invalid,

            StellaTag::Unit => StellaFieldKind::Invalid,

            StellaTag::Tuple => StellaFieldKind::Obj,

            StellaTag::Inl if idx == 0 => StellaFieldKind::Obj,
            StellaTag::Inl => StellaFieldKind::Invalid,

            StellaTag::Inr if idx == 0 => StellaFieldKind::Obj,
            StellaTag::Inr => StellaFieldKind::Invalid,

            StellaTag::Empty => StellaFieldKind::Invalid,

            StellaTag::Cons if idx < 2 => StellaFieldKind::Obj,
            StellaTag::Cons => StellaFieldKind::Invalid,
        }
    }
}

/// A wrapper around a pointer to a stella object.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjPtr(*mut StellaObj);

impl ObjPtr {
    /// Returns the number of fields in the object.
    ///
    /// # Safety
    /// The underlying pointer must point to a valid object.
    unsafe fn field_count(self) -> usize {
        let header = unsafe { (*self.0).header } as usize;

        (header & unsafe { FIELD_COUNT_MASK as usize }) >> 4
    }

    /// Returns the tag of the object.
    ///
    /// # Safety
    /// The underlying pointer must point to a valid object.
    unsafe fn tag(self) -> StellaTag {
        let header = unsafe { (*self.0).header } as usize;
        let tag = header & unsafe { TAG_MASK as usize };

        StellaTag::from_repr(tag).unwrap()
    }

    /// Returns the size of the object (counting both the header and the fields).
    ///
    /// # Safety
    /// The underlying pointer must point to a valid object.
    unsafe fn size(self) -> usize {
        let field_count = unsafe { self.field_count() };

        offset_of!(StellaObj, fields) + field_count * FIELD_SIZE
    }

    /// Computes a pointer to a field with the given index.
    ///
    /// # Safety
    /// The underlying pointer must point to a valid object, and the `idx` must not exceed the field
    /// count.
    unsafe fn field(self, idx: usize) -> *mut ObjPtr {
        unsafe {
            self.0
                .byte_add(offset_of!(StellaObj, fields) + idx * FIELD_SIZE)
                .cast()
        }
    }
}

/// Rounds `size` up so it has the given alignment.
fn align_up(size: usize, align: usize) -> usize {
    let misalignment = size % align;

    size + if misalignment > 0 {
        align - misalignment
    } else {
        0
    }
}

/// Rounds `size` down so it has the given alignment.
fn align_down(size: usize, align: usize) -> usize {
    size - size % align
}

/// A contiguous bounded chunk of memory; one of the two semi-spaces managed by the GC.
///
/// The memory is automatically deallocated once it's dropped.
#[derive(Default, Debug, Clone)]
struct Space {
    start: *mut u8,
    size: usize,
}

impl Space {
    /// Allocates a new semi-space no larger than `size`.
    fn alloc(size: usize) -> Self {
        let size = align_down(size.max(1), ALIGNMENT);

        if size == 0 {
            Self {
                start: ptr::null_mut(),
                size: 0,
            }
        } else {
            let layout = unsafe { Layout::from_size_align_unchecked(size, ALIGNMENT) };
            let start = unsafe { alloc(layout) };

            Self { start, size }
        }
    }

    /// Returns the pointer one past the last byte belonging to this semi-space.
    fn end(&self) -> *mut u8 {
        unsafe { self.start.byte_add(self.size) }
    }

    /// Checks if a pointer points to this semi-space.
    ///
    /// Note that `contains(end())` returns `false`.
    fn contains(&self, ptr: *mut u8) -> bool {
        !ptr.is_null() && (self.start..self.end()).contains(&ptr)
    }
}

impl Drop for Space {
    fn drop(&mut self) {
        if !self.start.is_null() {
            let layout = unsafe { Layout::from_size_align_unchecked(self.size, ALIGNMENT) };
            unsafe { dealloc(self.start, layout) };
            self.start = ptr::null_mut();
        }
    }
}

/// An enumeration of memory regions addresses may belong to.
#[derive(strum::Display, Debug, Clone, Copy, PartialEq, Eq)]
enum SpaceClass {
    /// A from-space.
    #[strum(to_string = "from{offset:+}")]
    From {
        /// An offset from the start of the from-space.
        offset: usize,
    },

    /// A to-space.
    #[strum(to_string = "to{offset:+}")]
    To {
        /// An offset from the start of the to-space.
        offset: usize,
    },

    /// Memory not managed by the GC.
    #[strum(to_string = "unmanaged")]
    Unmanaged,
}

/// Garbage collection statistics.
#[derive(Default, Debug, Clone, Copy)]
struct Stats {
    /// The number of field reads.
    reads: usize,

    /// The number of field writes.
    writes: usize,

    /// The number of field reads that triggered a read barrier.
    read_barriers: usize,

    /// The amount of memory allocated since the start of the program.
    all_time_allocated: usize,

    /// The number of allocated objects (i. e., calls to [`Gc::alloc`]) since the start of the
    /// program.
    all_time_allocated_objs: usize,

    /// The maximum amount of used memory managed by the GC.
    max_used: usize,

    /// The number of times garbage collection took place.
    ///
    /// Includes the partical GC cycle when garbage collection is in progress.
    gc_cycles: usize,
}

/// A copying semi-space garbage collector.
struct Gc {
    /// The from-space.
    ///
    /// If garbage collection is not currently underway, this field contains `None`.
    from_space: Option<Space>,

    /// The to-space.
    to_space: Space,

    /// The root stack.
    roots: Vec<*mut ObjPtr>,

    /// Whether a garbage collection cycle is currently underway.
    gc_in_progress: bool,

    /// The end of the scanned area in the to-space.
    scan: *mut u8,

    /// If GC is undergoing, the end of the area to scan.
    /// Otherwise, the place to allocate the next object at.
    next: *mut u8,

    /// If GC is undergoing, the place before which the next object is allocated.
    /// Otherwise, the end of the free area.
    limit: *mut u8,

    /// Garbage collection statistics.
    stats: Stats,
}

unsafe impl Send for Gc {}

impl Gc {
    /// Creates a new garbage collector instance.
    ///
    /// # Safety
    /// The external variables must have already been initialized to valid values.
    pub unsafe fn new() -> Self {
        let to_space = Space::alloc(usize::try_from(unsafe { max_alloc_size }).unwrap());
        let next = to_space.start;
        let limit = to_space.end();

        Self {
            from_space: None,
            to_space,

            roots: Default::default(),

            gc_in_progress: false,
            scan: Default::default(),
            next,
            limit,

            stats: Default::default(),
        }
    }

    /// Allocates a new object of the given size at `self.next`.
    ///
    /// Returns `None` if there's not enough free memory in the to-space.
    ///
    /// # Safety
    /// Must only be called when `self.gc_in_progress` is `false`.
    unsafe fn alloc_at_next(&mut self, size: usize) -> Option<ObjPtr> {
        if !self.limit.is_null() && self.next.wrapping_byte_add(size) < self.limit {
            let result = ObjPtr(self.next.cast());
            self.next = unsafe { self.next.byte_add(size) };

            return Some(result);
        }

        None
    }

    /// Records a new allocation for the stats.
    fn register_alloc(&mut self, size: usize) {
        self.stats.all_time_allocated += size;
        self.stats.all_time_allocated_objs += 1;
        self.stats.max_used = self.stats.max_used.max(self.used_memory());
    }

    /// Allocates a new object of the given size.
    ///
    /// Starts a GC cycle if it's deemed necessary.
    ///
    /// # Panics
    /// Panics if there's not enough free memory while GC is in progress.
    ///
    /// # Safety
    /// The size must be non-zero.
    pub unsafe fn alloc(&mut self, size: usize) -> ObjPtr {
        let size = align_up(size, ALIGNMENT);

        if !self.gc_in_progress {
            if let Some(result) = unsafe { self.alloc_at_next(size) } {
                self.register_alloc(size);

                return result;
            }

            unsafe { self.begin_gc() };
        }

        if self.limit.is_null()
            || self.next.is_null()
            || self.limit.wrapping_byte_sub(size) < self.next
        {
            panic!("out of memory");
        }

        let result = unsafe { self.limit.byte_sub(size) };
        self.limit = result;

        unsafe { self.run_gc(size) };
        self.register_alloc(size);

        ObjPtr(result.cast())
    }

    /// Starts a new GC cycle.
    ///
    /// # Safety
    /// This method must only be called if GC is not currently underway. All roots must have already
    /// been registered in the root stack.
    unsafe fn begin_gc(&mut self) {
        self.gc_in_progress = true;
        self.stats.gc_cycles += 1;

        let new_size = self.to_space.size;
        mem::swap(self.from_space.get_or_insert_default(), &mut self.to_space);

        if new_size != self.to_space.size {
            self.to_space = Space::alloc(new_size);
        }

        self.next = self.to_space.start;
        self.scan = self.to_space.start;
        self.limit = self.to_space.end();

        let roots = mem::take(&mut self.roots);

        for &root in &roots {
            unsafe { ptr::write(root, self.forward(*root)) };
        }

        self.roots = roots;
    }

    /// Continues the current GC cycle by scanning `n` bytes.
    ///
    /// # Safety
    /// This method must only be called during a GC cycle.
    unsafe fn run_gc(&mut self, n: usize) {
        let target = self.scan.wrapping_byte_add(n);

        while self.scan < self.next {
            if self.scan > target {
                return;
            }

            let ptr = ObjPtr(self.scan.cast());
            let field_count = unsafe { ptr.field_count() };

            for idx in 0..field_count {
                let field_ptr = unsafe { ptr.field(idx) };

                unsafe { ptr::write(field_ptr, self.forward(*field_ptr)) };
            }

            self.scan = unsafe { self.scan.byte_add(ptr.size()) };
        }

        self.gc_in_progress = false;
        self.from_space = None;
    }

    /// Forwards a pointer from the from-space to the to-space if necessary.
    ///
    /// Returns a pointer to the forwarded object, or `ptr` if forwarding is not applicable.
    ///
    /// # Safety
    /// If `ptr` points to the from-space, it must point to the start of a valid stella object with
    /// at least one field. The same requirement applies transitively to the contents of its fields.
    unsafe fn forward(&mut self, ptr: ObjPtr) -> ObjPtr {
        if self
            .from_space
            .as_ref()
            .is_some_and(|from_space| from_space.contains(ptr.0.cast()))
        {
            let mut result = unsafe { *ptr.field(0) };

            if !self.to_space.contains(result.0.cast()) {
                unsafe { self.chase(ptr) };
                result = unsafe { *ptr.field(0) };
            }

            assert!(self.to_space.contains(result.0.cast()));

            result
        } else {
            ptr
        }
    }

    /// Performs a semi-DFS walk forwarding pointers, starting with `ptr`.
    ///
    /// # Safety
    /// `ptr` must point to the start of a valid stella object in the from-space with at least one
    /// field. The same requirement applies transitively to the contents of its fields.
    unsafe fn chase(&mut self, mut ptr: ObjPtr) {
        loop {
            let wr = ObjPtr(self.next.cast());
            self.next = unsafe { self.next.wrapping_byte_add(ptr.size()) };

            if self.next > self.limit {
                panic!("out of memory");
            }

            let mut next = ObjPtr(ptr::null_mut());
            unsafe { ptr::copy(ptr.0, wr.0, 1) };

            for idx in 0..unsafe { ptr.field_count() } {
                let field = unsafe { *ptr.field(idx) };
                unsafe { ptr::write(wr.field(idx), field) };

                if self
                    .from_space
                    .as_ref()
                    .is_some_and(|from_space| from_space.contains(field.0.cast()))
                    && !self.to_space.contains(unsafe { *field.field(0) }.0.cast())
                {
                    next = field;
                }
            }

            unsafe { ptr::write(ptr.field(0), wr) };
            ptr = next;

            if ptr.0.is_null() {
                break;
            }
        }
    }

    /// Reads the value of a field of a stella object, forwarding it if necessary.
    ///
    /// # Safety
    /// `ptr` must point to a valid stella object. `field_idx` must be less than the field count.
    unsafe fn read_barrier(&mut self, ptr: ObjPtr, field_idx: usize) -> ObjPtr {
        self.stats.reads += 1;

        let mut result = unsafe { *ptr.field(field_idx) };

        if self.gc_in_progress
            && self
                .from_space
                .as_ref()
                .is_some_and(|from_space| from_space.contains(result.0.cast()))
        {
            unsafe {
                result = self.forward(result);
                ptr::write(ptr.field(field_idx), result);
            }

            self.stats.read_barriers += 1;
        }

        result
    }

    /// Records a write to a field of a GC-managed object.
    fn record_write(&mut self, ptr: ObjPtr) {
        match self.classify_space(ptr.0) {
            SpaceClass::From { .. } | SpaceClass::To { .. } => self.stats.writes += 1,
            SpaceClass::Unmanaged => {}
        }
    }

    /// Returns how much memory (in bytes) is used in the to-space.
    fn to_space_used_memory(&self) -> usize {
        unsafe {
            self.to_space.end().byte_offset_from_unsigned(self.limit)
                + self.next.byte_offset_from_unsigned(self.to_space.start)
        }
    }

    /// Returns how much free memory remains before the next GC cycle begins.
    fn free_memory(&self) -> usize {
        unsafe { self.limit.byte_offset_from_unsigned(self.next) }
    }

    /// Returns how much memory is used in the both semi-spaces.
    fn used_memory(&self) -> usize {
        let to_space_used = self.to_space_used_memory();

        self.from_space
            .as_ref()
            .map(|space| space.size)
            .unwrap_or(0)
            + to_space_used
    }

    /// Returns `true` if `ptr` has been forwarded to the to-space.
    ///
    /// # Safety
    /// `ptr` must point to a valid stella object.
    unsafe fn is_forwarded(&self, ptr: ObjPtr) -> bool {
        let field_count = unsafe { ptr.field_count() };

        field_count > 0
            && self
                .from_space
                .as_ref()
                .is_some_and(|from_space| from_space.contains(ptr.0.cast()))
            && self.to_space.contains(unsafe { *ptr.field(0) }.0.cast())
    }

    /// Determines the space class of the pointer.
    fn classify_space(&self, ptr: *mut StellaObj) -> SpaceClass {
        if let Some(from_space) = &self.from_space
            && from_space.contains(ptr.cast())
        {
            SpaceClass::From {
                offset: unsafe { ptr.byte_offset_from_unsigned(from_space.start) },
            }
        } else if self.to_space.contains(ptr.cast()) {
            SpaceClass::To {
                offset: unsafe { ptr.byte_offset_from_unsigned(self.to_space.start) },
            }
        } else {
            SpaceClass::Unmanaged
        }
    }

    /// Formats a stella object.
    ///
    /// If `display_fields` is `false`, the object's fields are elided from the output.
    ///
    /// # Safety
    /// The `ptr` must point to a valid stella object when [`Display::fmt`] is called.
    unsafe fn display_obj(&self, ptr: ObjPtr, display_fields: bool) -> impl Display {
        struct Fmt<'a> {
            gc: &'a Gc,
            ptr: ObjPtr,
            display_fields: bool,
        }

        impl Display for Fmt<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let tag = unsafe { self.ptr.tag() };
                let addr = self.ptr.0;

                let space = self.gc.classify_space(addr);
                let name = tag.to_string();
                let size = unsafe { self.ptr.size() };
                write!(f, "<{name} @ {addr:?} ({space}, {size} B)> {{")?;

                match unsafe { self.ptr.field_count() } {
                    0 => write!(f, "}}"),
                    _ if !self.display_fields => write!(f, "...}}"),

                    field_count => {
                        for idx in 0..field_count {
                            if idx > 0 {
                                write!(f, ", ")?;
                            } else {
                                write!(f, " ")?;
                            }

                            let field = unsafe { *self.ptr.field(idx) };
                            let field_addr = field.0;
                            let field_space = self.gc.classify_space(field.0.cast());

                            match tag.field_kind(idx) {
                                _ if idx == 0 && unsafe { self.gc.is_forwarded(self.ptr) } => {
                                    write!(f, "#{field_addr:?} ({field_space}, fwd)")?
                                }

                                StellaFieldKind::Raw => {
                                    write!(f, "#{field_addr:?} ({field_space})")?
                                }

                                StellaFieldKind::Invalid => write!(
                                    f,
                                    "#{field_addr:?} ({field_space}, **UNEXPECTED FIELD**)",
                                )?,

                                StellaFieldKind::Obj => {
                                    write!(f, "{}", unsafe { self.gc.display_obj(field, false) })?
                                }
                            }
                        }

                        write!(f, " }}")
                    }
                }
            }
        }

        Fmt {
            gc: self,
            ptr,
            display_fields,
        }
    }
}

/// A global instance of the garbage collector.
static GC: LazyLock<Mutex<Gc>> = LazyLock::new(|| Mutex::new(unsafe { Gc::new() }));

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn gc_alloc(size_in_bytes: usize) -> *mut c_void {
    unsafe { GC.lock().unwrap().alloc(size_in_bytes) }.0.cast()
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn gc_read_barrier(obj: ObjPtr, field_idx: c_int) -> *mut c_void {
    let result = unsafe {
        GC.lock()
            .unwrap()
            .read_barrier(obj, field_idx.try_into().unwrap())
    };

    result.0.cast()
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn gc_write_barrier(obj: ObjPtr, _field_idx: c_int, _value: ObjPtr) {
    GC.lock().unwrap().record_write(obj)
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn gc_push_root(root: *mut ObjPtr) {
    GC.lock().unwrap().roots.push(root);
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn gc_pop_root(root: *mut ObjPtr) {
    let popped = GC
        .lock()
        .unwrap()
        .roots
        .pop()
        .expect("popping from empty root stack");
    debug_assert_eq!(root, popped);
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn print_gc_alloc_stats() {
    let gc = GC.lock().unwrap();
    eprintln!(
        "  - All-time allocated: {} B ({} objects)",
        gc.stats.all_time_allocated, gc.stats.all_time_allocated_objs,
    );
    eprintln!("  - Used:");
    eprintln!("    - Currently {} B", gc.used_memory());
    eprintln!("    - Max: {} B", gc.stats.max_used);
    eprintln!(
        "  - GC cycles: {}{}",
        gc.stats.gc_cycles,
        if gc.gc_in_progress {
            " (currently in progress)"
        } else {
            ""
        },
    );
    eprintln!(
        "  - Reads: {} ({} barriers)",
        gc.stats.reads, gc.stats.read_barriers
    );
    eprintln!("  - Writes: {} (0 barriers)", gc.stats.writes);
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn print_gc_state() {
    let gc = GC.lock().unwrap();

    eprintln!("GC state:");

    if let Some(from_space) = &gc.from_space {
        let start = from_space.start;
        let end = from_space.end();

        eprintln!("  - From-space ({start:?}..{end:?}):");

        let mut addr = start;

        while addr < end {
            let ptr = ObjPtr(addr.cast());
            let offset = unsafe { addr.byte_offset_from_unsigned(start) };
            eprintln!("    - {addr:?} (from-space{offset:+}): {}", unsafe {
                gc.display_obj(ptr, true)
            });
            addr = unsafe { addr.byte_add(ptr.size()) };
        }

        eprintln!();
    }

    {
        let start = gc.to_space.start;
        let end = gc.to_space.end();
        eprintln!("  - To-space ({start:?}..{end:?}):");

        let mut addr = start;

        while addr < gc.next {
            let ptr = ObjPtr(addr.cast());
            let offset = unsafe { addr.byte_offset_from_unsigned(start) };
            eprintln!("    - {addr:?} (to-space{offset:+}): {}", unsafe {
                gc.display_obj(ptr, true)
            });
            addr = unsafe { addr.byte_add(ptr.size()) };
        }

        let free_start = gc.next;
        let free_end = gc.limit;

        if free_start < free_end {
            eprintln!("    - {free_start:?}..{free_end:?} free");
        }

        addr = gc.limit;

        while addr < end {
            let ptr = ObjPtr(addr.cast());
            let offset = unsafe { addr.byte_offset_from_unsigned(start) };
            eprintln!("    - {addr:?} (to-space{offset:+}): {}", unsafe {
                gc.display_obj(ptr, true)
            });
            addr = unsafe { addr.byte_add(ptr.size()) };
        }
    }

    eprintln!();

    if gc.gc_in_progress {
        eprintln!("  - Garbage collection currently in progress:");
        eprintln!("    - Scan pointer: {:?}", gc.scan);
        eprintln!("    - Next pointer: {:?}", gc.next);
        eprintln!("    - Limit pointer: {:?}", gc.limit);
    } else {
        eprintln!("  - Garbage collection currently not running");
    }

    eprintln!();

    if gc.roots.is_empty() {
        eprintln!("  - Roots: (none)");
    } else {
        eprintln!("  - Roots:");

        for &root in &gc.roots {
            let addr = unsafe { *root }.0;

            if gc.classify_space(addr.cast()) == SpaceClass::Unmanaged {
                eprintln!("    - **ILLEGAL** {root:?} points to {addr:?} (**unmanaged memory**)");
            } else {
                eprintln!("    - {root:?} points to {}", unsafe {
                    gc.display_obj(*root, true)
                });
            }
        }
    }

    eprintln!();
    eprintln!("  - Currently used: {} B", gc.used_memory());

    if let Some(from_space) = &gc.from_space {
        eprintln!(
            "    - From-space: {} B / {} B used, 0 B free",
            from_space.size, from_space.size,
        );
    }

    eprintln!(
        "    - To-space: {} B / {} B used, {} B free",
        gc.to_space_used_memory(),
        gc.to_space.size,
        gc.free_memory(),
    );

    eprintln!();
}

#[unsafe(no_mangle)]
#[nounwind]
pub unsafe extern "C" fn print_gc_roots() {
    let gc = GC.lock().unwrap();

    for &root in &gc.roots {
        let addr = unsafe { *root }.0;

        if gc.classify_space(addr.cast()) == SpaceClass::Unmanaged {
            eprintln!("**ILLEGAL** {root:?} points to {addr:?} (**unmanaged memory**)");
        } else {
            eprintln!("{root:?} points to {}", unsafe {
                gc.display_obj(*root, true)
            });
        }
    }
}
