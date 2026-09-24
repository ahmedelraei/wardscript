"""Runtime for Python modules compiled from Wardscript.

    from wardscript import runtime
    from wardscript.mock import MockModel

    runtime.configure(model=MockModel({"triage": {...}}))
"""

from . import runtime
from .errors import (
    AiOutputError,
    ApprovalDenied,
    BudgetExceeded,
    DecodeError,
    NoModelError,
    PanicError,
    Thrown,
    ToolError,
    TrustError,
    WardError,
)
from .model import AiRequest, Completion, Model
from .runtime import ApprovalRequest, configure
from .schema import decode, encode, json_schema
from .trust import Trusted
from .values import Some

__all__ = [
    "AiOutputError",
    "AiRequest",
    "ApprovalDenied",
    "ApprovalRequest",
    "BudgetExceeded",
    "Completion",
    "DecodeError",
    "Model",
    "NoModelError",
    "PanicError",
    "Some",
    "Thrown",
    "ToolError",
    "Trusted",
    "TrustError",
    "WardError",
    "configure",
    "decode",
    "encode",
    "json_schema",
    "runtime",
]
