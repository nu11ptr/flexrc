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
| Raw pointer APIs             | Missing      | No `into_raw`, `from_raw`, `as_ptr`, or manual strong-count APIs.      |
| Count inspection             | Missing      | No `strong_count`; weak counts are intentionally out of scope.         |
| COW/extraction APIs          | Missing      | No `try_unwrap`, `into_inner`, `make_mut`, or `unwrap_or_clone`.       |
| Uninit/zeroed allocation     | Partial      | Flex has slice uninit support, but names and bounds differ.            |
| Trait compatibility          | Sparse       | Many std trait impls are not present yet.                             |
| No-weak swap readiness       | Partial      | Fine for simple `new`/`clone`/`deref` use; still missing many strong-only APIs. |

## Stable Inherent Methods

| Std method                  | Std availability | Flex status | Compatibility notes |
| ---                         | ---              | ---         | --- |
| `new`                       | `Rc`, `Arc`      | Present     | Compatible shape: `new(T) -> Self`. |
| `new_uninit`                | `Rc`, `Arc`      | Missing     | Std creates `Rc<MaybeUninit<T>>` / `Arc<MaybeUninit<T>>`. |
| `new_zeroed`                | `Rc`, `Arc`      | Missing     | Stable in std; no scalar zeroed constructor in flex. |
| `pin`                       | `Rc`, `Arc`      | Missing     | Would return `Pin<Self>`. |
| `try_pin`                   | `Arc` only       | Missing     | Arc-only stable API. |
| `try_unwrap`                | `Rc`, `Arc`      | Missing     | Needs unique-ownership extraction. |
| `into_inner`                | `Rc`, `Arc`      | Missing     | Similar to `try_unwrap(...).ok()`. |
| `new_uninit_slice`          | `Rc`, `Arc`      | Partial     | Flex has `new_slice_uninit`, but the name differs and it currently requires `T: Copy`. |
| `new_zeroed_slice`          | `Rc`, `Arc`      | Missing     | No zeroed slice constructor. |
| `assume_init`               | `Rc`, `Arc`      | Partial     | Flex has slice `assume_init`; std has scalar and slice forms. |
| `from_raw`                  | `Rc`, `Arc`      | Missing     | Unsafe raw-pointer reconstruction API. |
| `into_raw`                  | `Rc`, `Arc`      | Missing     | Unsafe raw-pointer interop API. |
| `increment_strong_count`    | `Rc`, `Arc`      | Missing     | Requires raw-pointer count manipulation. |
| `decrement_strong_count`    | `Rc`, `Arc`      | Missing     | Requires raw-pointer count manipulation. |
| `as_ptr`                    | `Rc`, `Arc`      | Missing     | Should return a stable `*const T` data pointer. |
| `strong_count`              | `Rc`, `Arc`      | Missing     | Needs a public count query for each algorithm. |
| `get_mut`                   | `Rc`, `Arc`      | Present     | Same practical call shape for strong-only code. |
| `ptr_eq`                    | `Rc`, `Arc`      | Missing     | Straightforward to add using allocation identity. |
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
| Generic `CoerceUnsized` support | Custom smart-pointer unsizing is not implementable on stable Rust today; concrete conversions can still be added where useful. |

## Same Or Similar Names With Differences

| Name                  | Std behavior | Flex behavior | Compatibility concern |
| ---                   | ---          | ---           | --- |
| `get_mut`             | Associated function: `Rc::get_mut(&mut rc)` / `Arc::get_mut(&mut arc)` | Method: `rc.get_mut()`; also callable as `SmallRc::get_mut(&mut rc)` | Compatible for strong-only code; std has additional weak-ref failure cases that flex intentionally does not have. |
| `assume_init`         | Exists for scalar and slice `MaybeUninit` allocations | Exists only for `[MaybeUninit<T>]` allocations | Code using scalar `Rc::<T>::new_uninit().assume_init()` will not compile. |
| `get_mut_unchecked`   | Nightly-only in std 1.96.0 | Public unsafe flex method | Not a stable std-compatibility target yet; exposing it is extra API. |
| `new_uninit_slice`    | Std name for uninitialized slice allocation | Flex equivalent is named `new_slice_uninit` | Transparent swaps need the std name as an alias. |
| `from_slice`          | Not an inherent std method; std uses `From<&[T]> for Rc<[T]>` / `Arc<[T]>` | Flex inherent constructor for `[T]` when `str_deref` is disabled | Useful, but not source-compatible with std conversion code. |
| `from_str_ref`        | Std uses `From<&str> for Rc<str>` / `Arc<str>` | Flex creates `FlexRc<[u8]>`; with `str_deref`, it derefs as `str` | Type shape differs from `Rc<str>` / `Arc<str>`. |
| `from_ref`            | Std stable API does not have this inherent method | Flex clones from `&T` into a new allocation | Extra convenience method, not a std replacement method. |

## Flex-Only Inherent Methods

| Flex method        | Applies to | Purpose | Std equivalent |
| ---                | ---        | ---     | --- |
| `try_into_other`   | All flex types | Consuming local/shared conversion; returns `Err(self)` if in-place conversion is not possible. | None |
| `into_other`       | All flex types where `T: Clone` | Consuming conversion; clones `T` if in-place conversion is not possible. | None |
| `try_to_other`     | All flex types | Non-consuming local/shared conversion; returns `Err(&self)` if in-place conversion is not possible. | None |
| `to_other`         | All flex types where `T: Clone` | Non-consuming conversion; clones `T` if in-place conversion is not possible. | None |
| `from_ref`         | Sized `T: Clone` | Creates a new allocation by cloning from `&T`. | No stable inherent std equivalent |
| `from_slice`       | `[T]` where `T: Copy`, unless `str_deref` is enabled | Creates a slice allocation from a slice. | `From<&[T]> for Rc<[T]>` / `Arc<[T]>` |
| `from_str_ref`     | `[u8]` | Creates bytes from string data; may deref as `str` under `str_deref`. | `From<&str> for Rc<str>` / `Arc<str>` |
| `new_slice_uninit` | `[T]` where `T: Copy` | Creates uninitialized slice storage. | `new_uninit_slice`, without the same name or bounds |

## Trait Implementations

| Trait / conversion family            | Std `Rc` | Std `Arc` | Flex status | Compatibility notes |
| ---                                  | ---      | ---       | ---         | --- |
| `Clone`                              | Yes      | Yes       | Present     | Compatible. |
| `Deref`                              | Yes      | Yes       | Present     | Mostly compatible; `str_deref` changes `[u8]` deref behavior. |
| `Drop`                               | Yes      | Yes       | Present     | Compatible ownership behavior. |
| `Send` / `Sync`                      | `Rc`: no | `Arc`: conditional | Present for shared types | `SmallArc`, `HybridArc`, and `ThreadArc` are `Send + Sync` when `T: Send + Sync`; local types are not. |
| `AsRef<T>`                           | Yes      | Yes       | Missing     | Common ergonomic gap. |
| `Borrow<T>`                          | Yes      | Yes       | Missing     | Affects map/set lookup ergonomics. |
| `Debug`                              | Yes      | Yes       | Missing     | Common diagnostics gap. |
| `Display`                            | Yes      | Yes       | Missing     | Common formatting gap. |
| `Default`                            | Yes      | Yes       | Missing     | Std supports `T: Default`, `[T]`, `str`, and `CStr` variants. |
| `Hash`                               | Yes      | Yes       | Missing     | Needed for hash maps/sets by value. |
| `PartialEq` / `Eq`                   | Yes      | Yes       | Missing     | Needed for ordinary comparisons. |
| `PartialOrd` / `Ord`                 | Yes      | Yes       | Missing     | Needed for ordering comparisons. |
| `Pointer` formatting                 | Yes      | Yes       | Missing     | Affects `format!("{:p}", rc)`. |
| `From<T>`                            | Yes      | Yes       | Missing     | Std supports `Rc::from(value)` / `Arc::from(value)`. |
| `From<Box<T>>`                       | Yes      | Yes       | Missing     | Useful allocation conversion. |
| `From<&[T]>` / `From<&mut [T]>`       | Yes      | Yes       | Missing     | Flex has inherent `from_slice`, but not the trait conversions. |
| `From<[T; N]>`                       | Yes      | Yes       | Missing     | Needed for array-to-slice allocation conversions. |
| `From<Vec<T>>`                       | Yes      | Yes       | Missing     | Needed for vec-to-slice allocation conversions. |
| `From<&str>` / `From<String>`         | Yes      | Yes       | Design decision | Std produces `Rc<str>` / `Arc<str>`; flex currently uses `[u8]` plus optional `str_deref`. |
| `FromIterator<T> for _<[T]>`          | Yes      | Yes       | Missing     | Needed for `collect::<Rc<[T]>>()` style code. |
| `TryFrom<_<[T]>> for _<[T; N]>`       | Yes      | Yes       | Missing     | Slice-to-array allocation conversion. |
| `Unpin`                              | Yes      | Yes       | Likely auto | Should be checked explicitly if compatibility is pursued. |
| `UnwindSafe` / `RefUnwindSafe`        | Yes      | Yes       | Not audited | Should be checked explicitly if compatibility is pursued. |
| `Error`                              | No       | Yes       | Missing     | `Arc<T>` implements `Error` when `T: Error + ?Sized`. |
| Waker conversions                    | Yes      | Yes       | Missing     | Std has `Rc<W>: Into<LocalWaker/RawWaker>` and `Arc<W>: Into<Waker/RawWaker>` for wake traits. |

## Recommended Compatibility Roadmap

| Priority | Work | Why |
| ---      | ---  | --- |
| 1 | Add low-risk trait impls: `Debug`, `Display`, `AsRef`, `Borrow`, comparisons, `Hash`, `Pointer`. | Removes a lot of everyday source incompatibility with little algorithmic risk. |
| 2 | Add count and identity APIs: `as_ptr`, `ptr_eq`, `strong_count`. | Common and useful; does not require weak refs or raw ownership transfer. |
| 3 | Add extraction/COW APIs: `try_unwrap`, `into_inner`, `unwrap_or_clone`, `make_mut`. | Important for std-like ownership workflows. |
| 4 | Add std-named uninit APIs: `new_uninit`, `new_uninit_slice`, scalar `assume_init`, and maybe `new_zeroed`/`new_zeroed_slice`. | Makes modern std allocation patterns compile. |
| 5 | Add conversion trait impls: `From<T>`, `From<Box<T>>`, slice/string/vector conversions, `FromIterator`. | Big source-compatibility win, especially for collection code. |
| 6 | Add raw pointer APIs only after a safety design pass. | These APIs expose allocation layout and count invariants directly. |
| 7 | Decide the string DST story. | Current `[u8]` plus `str_deref` design is useful but not source-identical to `Rc<str>` / `Arc<str>`. |
