# flexrc

[![Crates.io](https://img.shields.io/crates/v/flexrc.svg)](https://crates.io/crates/flexrc)
[![Documentation](https://docs.rs/flexrc/badge.svg)](https://docs.rs/flexrc)
[![CI](https://github.com/nu11ptr/flexrc/actions/workflows/ci.yml/badge.svg)](https://github.com/nu11ptr/flexrc/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/flexrc.svg)](#license)

`flexrc` is a crate that provides alternate `Rc`/`Arc`-style types. Its main purpose is to allow cheap, in place conversions between `Rc` and `Arc` types, when possible. This allows using cheap non-atomic counter clones in single threaded situations, but falling backing to atomic counted clones when necessary. It achieves this by using the same memory layout for both `Rc` and `Arc` types with the hybrid and thread tracking types holding both atomic and non-atomic counters simultaneously. Additionally, the thread types also track the current thread allowing for further in place conversion opportunties.

It was inspired by the [hybrid-rc](https://crates.io/crates/hybrid-rc) crate and the ["Biased reference counting: minimizing atomic operations in garbage collection"](https://dl.acm.org/doi/10.1145/3243176.3243195) paper.

The crate provides three families:

- `LocalRc<T>` / `SharedRc<T>`: regular local or shared reference counting. In-place conversion only succeeds when the allocation is unique.
- `LocalHybridRc<T>` / `SharedHybridRc<T>`: hybrid reference counting. Local and shared handles can coexist for the same allocation.
- `LocalThreadRc<T>` / `SharedThreadRc<T>`: thread-tracked hybrid reference counting. This adds same-thread recovery of local handles from shared handles when `track_threads` is enabled.

The public API is safe; the crate uses internal unsafe code to manage allocation layout, metadata reinterpretation, and reference-count transitions. It has not yet undergone strenuous testing yet in real world code and should be evaluated carefully before considering it for production use.

## Type Comparison

Metadata overhead is the allocation header metadata. It does not include the pointed-to `T`, allocator padding, or the handle pointer itself.

| Type                                      | Metadata overhead                                                     | Weak refs | Rc to Arc    | Rc into Arc                                        | Arc to Rc                                                                                    | Arc into Rc                                                       |
| ---                                       | ---:                                                                  | ---       | ---          | ---                                                | ---                                                                                          | ---                                                      |
| `std::rc::Rc<T>` / `std::sync::Arc<T>`    | 2 words (32-bits: 8 bytes, 64-bit: 16 bytes)                          | yes       | N/A          | N/A                                                | N/A                                                                                          | N/A                                                        |
| `LocalRc<T>` / `SharedRc<T>`              | 1 word (32-bits: 4 bytes, 64-bit: 8 bytes), `small_counters`: 4 bytes | no        | Clones `T`   | **Unique**: In place<br>**Non-unique**: Clones `T` | Clones `T`                                                                                   | **Unique**: In place<br>**Non-unique**: Clones `T`                                               |
| `LocalHybridRc<T>` / `SharedHybridRc<T>`  | 8 bytes                                                               | no        | In place     | In place                                           | **Rc count = 0**: In place<br>**Rc count &gt; 0**: Clones `T`                                | **Rc count = 0**: In place<br>**Rc count &gt; 0**: Clones `T`                               |
| `LocalThreadRc<T>` / `SharedThreadRc<T>`  | 1 word (32-bits: 4 bytes, 64-bit: 8 bytes) + 8 bytes                  | no        | In place     | In place                                           | **Rc count = 0 OR same thread**: in place<br>**Rc count &gt; 0 OR other thread**: Clones `T` | **Rc count = 0 OR same thread**: in place<br>**Rc count &gt; 0 OR other thread**: Clones `T` |

## Features

- `std` *(default)*: enables standard-library support and process abort on counter overflow.
- `track_threads` *(default, implies `std`)*: enables `LocalThreadRc<T>` and `SharedThreadRc<T>` types.
- `small_counters`: uses smaller counters for `LocalRc<T>` and `SharedRc<T>`.
- `str_deref`: lets `FlexRc<_, _, [u8]>` created from string data dereference as `str`.

Disable default features for `no_std` plus `alloc` use:

```toml
flexrc = { version = "0.1", default-features = false }
```

Use `std` without the thread-tracked family:

```toml
flexrc = { version = "0.1", default-features = false, features = ["std"] }
```

## Testing

The crate is tested using a typical Rust test suite, loom tests for concurrent reference-count transitions, and Miri with strict provenance flags.

To run the loom tests locally:

```sh
RUSTFLAGS='--cfg loom' cargo test -p flexrc --test loom_test
```

## License

Licensed under either of:

- Apache License, Version 2.0
- MIT license
