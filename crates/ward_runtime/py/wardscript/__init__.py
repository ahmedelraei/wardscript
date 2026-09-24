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
    BudgetUnenforceable,
    DecodeError,
    NoModelError,
    PanicError,
    Thrown,
    ToolError,
    TrustError,
    WardError,
)
from .model import AiRequest, Completion, Model, StreamChunk, StreamingModel
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
    "BudgetUnenforceable",
    "Completion",
    "DecodeError",
    "Model",
    "NoModelError",
    "PanicError",
    "Some",
    "StreamChunk",
    "StreamingModel",
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
