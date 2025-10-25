# agglutinator
A single-threaded incremental copying semi-space garbage collector for Stella.

## Build
To build `agglutinator`, use Cargo:

```
$ cargo build
```

This will produce a static library in `target/debug` that you can link with when building a stella executable:

```
$ gcc -DMAX_ALLOC_SIZE=4000 -DSTELLA_GC_STATS -DSTELLA_RUNTIME_STATS -std=c11 -g -iquote . stella/gc.c stella/runtime.c target/debug/libagglutinator.a stella-program.c -o stella-program
```

Change the value of `MAX_ALLOC_SIZE` to set the maximum size of a single semi-space (in effect limiting the total amount of GC-managed memory to twice the value).
Any integer literal is allowed as long as it fits into 64 bits, though setting it to 0 probably wouldn't be useful.

- You can also leave it out entirely, and it'll be set to a default value.

## FFI
`agglutinator` provides an implementation of all symbols declared in `stella/gc.h`.
The print functions output to `stderr`.

<details>

<summary><em>Output of <code>print_gc_alloc_stats()</code>.</em></summary>

```
- All-time allocated: 31760 B (1978 objects)
- Used:
  - Currently 23216 B
  - Max: 45896 B
- GC cycles: 1
- Reads: 2025 (0 barriers)
- Writes: 7 (0 barriers)
```

</details>

<details>

<summary><em>Output of <code>print_gc_state()</code>.</em></summary>

```
GC state:
  - From-space (0x56173295b5a0..0x56173295b668):
    - 0x56173295b5a0 (from-space+0): <succ @ 0x56173295b5a0 (from+0, 16 B)> { #0x56173295b380 (to+0, fwd) }
    - 0x56173295b5b0 (from-space+16): <succ @ 0x56173295b5b0 (from+16, 16 B)> { #0x56173295b390 (to+16, fwd) }
    - 0x56173295b5c0 (from-space+32): <fn @ 0x56173295b5c0 (from+32, 24 B)> { #0x56173295b3a0 (to+32, fwd), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x56173295b5d8 (from-space+56): <ref @ 0x56173295b5d8 (from+56, 16 B)> { #0x56173295b3b8 (to+56, fwd) }
    - 0x56173295b5e8 (from-space+72): <succ @ 0x56173295b5e8 (from+72, 16 B)> { #0x56173295b3c8 (to+72, fwd) }
    - 0x56173295b5f8 (from-space+88): <succ @ 0x56173295b5f8 (from+88, 16 B)> { #0x56173295b3d8 (to+88, fwd) }
    - 0x56173295b608 (from-space+104): <fn @ 0x56173295b608 (from+104, 24 B)> { #0x56173295b3e8 (to+104, fwd), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x56173295b620 (from-space+128): <zero @ 0x56173295b620 (from+128, 8 B)> {}
    - 0x56173295b628 (from-space+136): <succ @ 0x56173295b628 (from+136, 16 B)> { #0x56173295b410 (to+144, fwd) }
    - 0x56173295b638 (from-space+152): <succ @ 0x56173295b638 (from+152, 16 B)> { #0x56173295b400 (to+128, fwd) }
    - 0x56173295b648 (from-space+168): <fn @ 0x56173295b648 (from+168, 32 B)> { #0x561712593850 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...}, <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }

  - To-space (0x56173295b380..0x56173295b448):
    - 0x56173295b380 (to-space+0): <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - 0x56173295b390 (to-space+16): <succ @ 0x56173295b390 (to+16, 16 B)> { <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }
    - 0x56173295b3a0 (to-space+32): <fn @ 0x56173295b3a0 (to+32, 24 B)> { #0x561712593e00 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x56173295b3b8 (to-space+56): <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - 0x56173295b3c8 (to-space+72): <succ @ 0x56173295b3c8 (to+72, 16 B)> { <succ @ 0x56173295b5f8 (from+88, 16 B)> {...} }
    - 0x56173295b3d8 (to-space+88): <succ @ 0x56173295b3d8 (to+88, 16 B)> { <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }
    - 0x56173295b3e8 (to-space+104): <fn @ 0x56173295b3e8 (to+104, 24 B)> { #0x561712593a20 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x56173295b400 (to-space+128): <succ @ 0x56173295b400 (to+128, 16 B)> { <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }
    - 0x56173295b410 (to-space+144): <succ @ 0x56173295b410 (to+144, 16 B)> { <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }
    - 0x56173295b420..0x56173295b438 free
    - 0x56173295b438 (to-space+184): <succ @ 0x56173295b438 (to+184, 16 B)> { <succ @ 0x56173295b410 (to+144, 16 B)> {...} }

  - Garbage collection currently in progress:
    - Scan pointer: 0x56173295b3a0
    - Next pointer: 0x56173295b420
    - Limit pointer: 0x56173295b438

  - Roots:
    - **ILLEGAL** 0x7ffe89c2a328 points to 0x561712684060 (**unmanaged memory**)
    - 0x7ffe89c2a330 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - 0x7ffe89c2a318 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - 0x7ffe89c2a2a0 points to <fn @ 0x56173295b3a0 (to+32, 24 B)> { #0x561712593e00 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x7ffe89c2a2a8 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a2b0 points to 0x561712684070 (**unmanaged memory**)
    - 0x7ffe89c2a2b8 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - 0x7ffe89c2a2c0 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - 0x7ffe89c2a298 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a220 points to 0x561712593e00 (**unmanaged memory**)
    - 0x7ffe89c2a228 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a230 points to 0x5617126840b8 (**unmanaged memory**)
    - 0x7ffe89c2a238 points to <fn @ 0x56173295b3e8 (to+104, 24 B)> { #0x561712593a20 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x7ffe89c2a240 points to <fn @ 0x56173295b3e8 (to+104, 24 B)> { #0x561712593a20 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x7ffe89c2a208 points to <succ @ 0x56173295b380 (to+0, 16 B)> { <succ @ 0x56173295b390 (to+16, 16 B)> {...} }
    - 0x7ffe89c2a218 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a1c8 points to 0x561712684bc8 (**unmanaged memory**)
    - **ILLEGAL** 0x7ffe89c2a1c0 points to 0x5617126840b8 (**unmanaged memory**)
    - 0x7ffe89c2a1b8 points to <fn @ 0x56173295b3e8 (to+104, 24 B)> { #0x561712593a20 (unmanaged), <ref @ 0x56173295b5d8 (from+56, 16 B)> {...} }
    - 0x7ffe89c2a140 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a148 points to 0x33295b430 (**unmanaged memory**)
    - 0x7ffe89c2a150 points to <succ @ 0x56173295b400 (to+128, 16 B)> { <zero @ 0x561712684bc8 (unmanaged, 8 B)> {} }
    - 0x7ffe89c2a158 points to <succ @ 0x56173295b438 (to+184, 16 B)> { <succ @ 0x56173295b410 (to+144, 16 B)> {...} }
    - 0x7ffe89c2a160 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a128 points to 0x5617126840b8 (**unmanaged memory**)
    - 0x7ffe89c2a130 points to <ref @ 0x56173295b3b8 (to+56, 16 B)> { <succ @ 0x56173295b5e8 (from+72, 16 B)> {...} }
    - **ILLEGAL** 0x7ffe89c2a138 points to 0x561712684bc8 (**unmanaged memory**)

  - Currently used: 376 B
    - From-space: 200 B / 200 B used, 0 B free
    - To-space: 176 B / 200 B used, 24 B free
```

</details>
