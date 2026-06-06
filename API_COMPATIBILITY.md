# Rc/Arc API Compatibility

This compares `flexrc` against the stable inherent `std::rc::Rc` and
`std::sync::Arc` APIs available in `rustc 1.96.0`.

The comparison assumes the goal is a mostly transparent swap:

- `SmallRc<T>`, `HybridRc<T>`, and `ThreadRc<T>` are Rc-like local types.
- `SmallArc<T>`, `HybridArc<T>`, and `ThreadArc<T>` are Arc-like shared types.
- All flex types share the same `FlexRc` inherent methods, except
  `ThreadRc<T>` / `ThreadArc<T>` require the `track_threads` feature.
- Weak references are intentionally out of scope. APIs that only exist to
  create, inspect, or upgrade `Weak` pointers are not compatibility targets.

## Summary

| Area                         | Status       | Notes                                                                 |
| ---                          | ---          | ---                                                                   |
| Basic construction           | Good         | `new` is compatible.                                                  |
| Clone/drop/deref             | Good         | `Clone`, `Drop`, and `Deref` are implemented.                         |
| Mutable access               | Good         | `get_mut` exists; without weak refs, uniqueness only needs to consider strong handles. |
| Rc <-> Arc style conversion  | Extra        | Flex adds `try_into_other`, `into_other`, `try_to_other`, and `to_other`. |
| Raw pointer APIs             | Partial      | `as_ptr` exists; ownership-transfer and manual-count raw APIs are still missing. |
| Count inspection             | Deferred     | No `strong_count`; exact hybrid shared-side counts are not a simple safe read. |
| COW/extraction APIs          | Missing      | No `try_unwrap`, `into_inner`, `make_mut`, or `unwrap_or_clone`.       |
| Uninit/zeroed allocation     | Good         | Scalar and slice `new_uninit`, `new_zeroed`, and `assume_init` are present. |
| Trait compatibility          | Partial      | Common delegating traits are present; conversion traits remain incomplete. |
| No-weak swap readiness       | Partial      | Fine for simple `new`/`clone`/`deref` use; still missing many strong-only APIs. |

## Stable Inherent Methods

| Std method                  | Std availability | Flex status | Compatibility notes |
| ---                         | ---              | ---         | --- |
| `new`                       | `Rc`, `Arc`      | Present     | Compatible shape: `new(T) -> Self`. |
| `new_uninit`                | `Rc`, `Arc`      | Present     | Creates `FlexRc<MaybeUninit<T>>`. |
| `new_zeroed`                | `Rc`, `Arc`      | Present     | Creates zeroed `FlexRc<MaybeUninit<T>>`. |
| `pin`                       | `Rc`, `Arc`      | Present     | Returns `Pin<Self>`. |
| `try_unwrap`                | `Rc`, `Arc`      | Missing     | Needs unique-ownership extraction. |
| `into_inner`                | `Rc`, `Arc`      | Missing     | Similar to `try_unwrap(...).ok()`. |
| `new_uninit_slice`          | `Rc`, `Arc`      | Present     | Std-compatible name; `new_slice_uninit` remains as a flex alias. |
| `new_zeroed_slice`          | `Rc`, `Arc`      | Present     | Creates zeroed `[MaybeUninit<T>]` slice storage. |
| `assume_init`               | `Rc`, `Arc`      | Present     | Scalar and slice forms are present. |
| `from_raw`                  | `Rc`, `Arc`      | Missing     | Unsafe raw-pointer reconstruction API. |
| `into_raw`                  | `Rc`, `Arc`      | Missing     | Unsafe raw-pointer interop API. |
| `increment_strong_count`    | `Rc`, `Arc`      | Missing     | Requires raw-pointer count manipulation. |
| `decrement_strong_count`    | `Rc`, `Arc`      | Missing     | Requires raw-pointer count manipulation. |
| `as_ptr`                    | `Rc`, `Arc`      | Present     | Returns a stable `*const T` data pointer. |
| `strong_count`              | `Rc`, `Arc`      | Deferred    | Exact hybrid shared-side counts would require reading non-atomic local counters across threads. |
| `get_mut`                   | `Rc`, `Arc`      | Present     | Same practical call shape for strong-only code. |
| `ptr_eq`                    | `Rc`, `Arc`      | Present     | Uses allocation identity. |
| `make_mut`                  | `Rc`, `Arc`      | Missing     | Requires clone-on-write behavior. |
| `unwrap_or_clone`           | `Rc`, `Arc`      | Missing     | Requires extraction when unique, clone otherwise. |
| `downcast`                  | `Rc`, `Arc`      | Missing     | Applies to `dyn Any` allocations. Arc version requires `Any + Send + Sync`. |

## Out Of Scope APIs

These stable std APIs are intentionally not compatibility targets under the
no-weak-reference design.

| API                            | Reason |
| ---                            | --- |
| `new_cyclic`                   | Its constructor closure receives a `Weak<T>`. |
| `downgrade`                    | Creates a `Weak<T>`. |
| `weak_count`                   | Only meaningful when weak refs exist. |
| `Weak<T>`                      | Weak refs are intentionally unsupported. |
| `Weak::new`                    | Weak refs are intentionally unsupported. |
| `Weak::upgrade`                | Weak refs are intentionally unsupported. |
| `Weak::as_ptr`                 | Weak refs are intentionally unsupported. |
| `Weak::into_raw`               | Weak refs are intentionally unsupported. |
| `Weak::from_raw`               | Weak refs are intentionally unsupported. |
| `Weak::ptr_eq`                 | Weak refs are intentionally unsupported. |
| `Weak::strong_count`           | Weak refs are intentionally unsupported. |
| `Weak::weak_count`             | Weak refs are intentionally unsupported. |
| `Arc::try_pin`                 | Nightly-only in std 1.96.0 behind `allocator_api`; not a stable compatibility target. |
| Generic `CoerceUnsized` support | Custom smart-pointer unsizing is not implementable on stable Rust today; concrete conversions can still be added where useful. |

## Same Or Similar Names With Differences

| Name                  | Std behavior | Flex behavior | Compatibility concern |
| ---                   | ---          | ---           | --- |
| `get_mut`             | Associated function: `Rc::get_mut(&mut rc)` / `Arc::get_mut(&mut arc)` | Method: `rc.get_mut()`; also callable as `SmallRc::get_mut(&mut rc)` | Compatible for strong-only code; std has additional weak-ref failure cases that flex intentionally does not have. |
| `get_mut_unchecked`   | Nightly-only in std 1.96.0 | Public unsafe flex method | Not a stable std-compatibility target yet; exposing it is extra API. |
| `from_slice`          | Not an inherent std method; std uses `From<&[T]> for Rc<[T]>` / `Arc<[T]>` | Flex inherent constructor for `[T]` | Useful, but not source-compatible with std conversion code. |
| `from_ref`            | Std stable API does not have this inherent method | Flex clones from `&T` into a new allocation | Extra convenience method, not a std replacement method. |

## Flex-Only Inherent Methods

| Flex method        | Applies to | Purpose | Std equivalent |
| ---                | ---        | ---     | --- |
| `try_into_other`   | All flex types | Consuming local/shared conversion; returns `Err(self)` if in-place conversion is not possible. | None |
| `into_other`       | All flex types where `T: Clone` | Consuming conversion; clones `T` if in-place conversion is not possible. | None |
| `try_to_other`     | All flex types | Non-consuming local/shared conversion; returns `Err(&self)` if in-place conversion is not possible. | None |
| `to_other`         | All flex types where `T: Clone` | Non-consuming conversion; clones `T` if in-place conversion is not possible. | None |
| `from_ref`         | Sized `T: Clone` | Creates a new allocation by cloning from `&T`. | No stable inherent std equivalent |
| `from_slice`       | `[T]` where `T: Copy` | Creates a slice allocation from a slice. | `From<&[T]> for Rc<[T]>` / `Arc<[T]>` |
| `new_slice_uninit` | `[T]` | Alias for `new_uninit_slice`. | `new_uninit_slice` |

## Trait Implementations

| Trait / conversion family            | Std `Rc` | Std `Arc` | Flex status | Compatibility notes |
| ---                                  | ---      | ---       | ---         | --- |
| `Clone`                              | Yes      | Yes       | Present     | Compatible. |
| `Deref`                              | Yes      | Yes       | Present     | Compatible target shape: flex derefs to `T`, including native `str`. |
| `Drop`                               | Yes      | Yes       | Present     | Compatible ownership behavior. |
| `Send` / `Sync`                      | `Rc`: no | `Arc`: conditional | Present for shared types | `SmallArc`, `HybridArc`, and `ThreadArc` are `Send + Sync` when `T: Send + Sync`; local types are not. |
| `AsRef<T>`                           | Yes      | Yes       | Present     | Delegates to the contained value. |
| `Borrow<T>`                          | Yes      | Yes       | Present     | Delegates to the contained value. |
| `Debug`                              | Yes      | Yes       | Present     | Delegates to the contained value. |
| `Display`                            | Yes      | Yes       | Present     | Delegates to the contained value. |
| `Default`                            | Yes      | Yes       | Partial     | Present for `T: Default`; std also has dedicated `[T]`, `str`, and `CStr` defaults. |
| `Hash`                               | Yes      | Yes       | Present     | Delegates to the contained value. |
| `PartialEq` / `Eq`                   | Yes      | Yes       | Present     | Delegates to the contained value for the same flex handle type; std has broader cross-type comparison impls. |
| `PartialOrd` / `Ord`                 | Yes      | Yes       | Present     | Delegates to the contained value for the same flex handle type; std has broader cross-type partial-order impls. |
| `Pointer` formatting                 | Yes      | Yes       | Present     | Formats the data pointer. |
| `From<T>`                            | Yes      | Yes       | Present     | Equivalent to `new(value)`. |
| `From<Box<T>>`                       | Yes      | Yes       | Partial     | Present for `Box<str>`; generic boxed conversions are still missing. |
| `From<&[T]>` / `From<&mut [T]>`       | Yes      | Yes       | Missing     | Flex has inherent `from_slice`, but not the trait conversions. |
| `From<[T; N]>`                       | Yes      | Yes       | Missing     | Needed for array-to-slice allocation conversions. |
| `From<Vec<T>>`                       | Yes      | Yes       | Missing     | Needed for vec-to-slice allocation conversions. |
| `From<&str>` / `From<String>`         | Yes      | Yes       | Present     | Produces native `FlexRc<str>` handles, matching std's type shape. |
| `FromIterator<T> for _<[T]>`          | Yes      | Yes       | Missing     | Needed for `collect::<Rc<[T]>>()` style code. |
| `TryFrom<_<[T]>> for _<[T; N]>`       | Yes      | Yes       | Missing     | Slice-to-array allocation conversion. |
| `Unpin`                              | Yes      | Yes       | Likely auto | Should be checked explicitly if compatibility is pursued. |
| `UnwindSafe` / `RefUnwindSafe`        | Yes      | Yes       | Not audited | Should be checked explicitly if compatibility is pursued. |
| `Error`                              | No       | Yes       | Missing     | `Arc<T>` implements `Error` when `T: Error + ?Sized`. |
| Waker conversions                    | Yes      | Yes       | Missing     | Std has `Rc<W>: Into<LocalWaker/RawWaker>` and `Arc<W>: Into<Waker/RawWaker>` for wake traits. |

## Recommended Compatibility Roadmap

| Priority | Work | Why |
| ---      | ---  | --- |
| 1 | Add extraction/COW APIs: `try_unwrap`, `into_inner`, `unwrap_or_clone`, `make_mut`. | Important for std-like ownership workflows, but needs careful deallocation/move-out handling. |
| 2 | Add conversion trait impls: generic `From<Box<T>>`, slice/vector conversions, `FromIterator`. | Big source-compatibility win, especially for collection code. |
| 3 | Add raw pointer APIs only after a safety design pass. | These APIs expose allocation layout and count invariants directly. |
| 4 | Revisit `strong_count` only with an explicit hybrid semantics decision. | Exact counts are not a few-line method for shared hybrid handles because local counts are non-atomic. |
