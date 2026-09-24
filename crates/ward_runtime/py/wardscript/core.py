"""The runtime core: the Rust `_core` module when the package was built with it, or the
pure-Python `_core_py` with the same interface. `IMPLEMENTATION` says which."""

try:
    from . import _core as impl  # type: ignore[attr-defined]
except ImportError:
    from . import _core_py as impl

IMPLEMENTATION: str = impl.IMPLEMENTATION
Budget = impl.Budget
Recorder = impl.Recorder
digest = impl.digest
leaves = impl.leaves

__all__ = ["IMPLEMENTATION", "Budget", "Recorder", "digest", "leaves"]
