"""A minimal MCP client over stdio, so `import mcp "gmail" as mail` can call a real MCP
server. Servers are configured like other MCP clients do, in `mcp.json`:

    {"mcpServers": {"gmail": {"command": "python3", "args": ["gmail_server.py"]}}}

    from wardscript import mcp, runtime

    runtime.configure(tools=mcp.load_config("mcp.json"))

Each server starts on its first call and stops with `close()` or at exit. A tool result
is its `structuredContent` when there is one, else its text content; a result with
`isError` raises `Thrown` with the text, which Wardscript code can catch.
"""

from __future__ import annotations

import atexit
import json
import os
import queue
import subprocess
import threading
from typing import Any, Mapping, Sequence

from .errors import Thrown, ToolError

PROTOCOL_VERSION = "2025-06-18"
_EOF = object()


class Server:
    def __init__(
        self,
        command: str,
        args: Sequence[str] = (),
        env: Mapping[str, str] | None = None,
        cwd: str | None = None,
        timeout: float = 60.0,
        name: str | None = None,
    ) -> None:
        self.command = [command, *args]
        self.env = dict(env or {})
        self.cwd = cwd
        self.timeout = timeout
        self.name = name or os.path.basename(command)
        self._proc: subprocess.Popen[str] | None = None
        self._lines: queue.Queue[Any] = queue.Queue()
        self._lock = threading.Lock()
        self._next_id = 0

    def __repr__(self) -> str:
        return f"mcp.Server({self.name!r})"

    # -- lifecycle --

    def _start(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            return
        try:
            self._proc = subprocess.Popen(
                self.command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=None,
                text=True,
                encoding="utf-8",
                bufsize=1,
                cwd=self.cwd,
                env={**os.environ, **self.env},
            )
        except OSError as e:
            raise ToolError(f"MCP server `{self.name}` didn't start: {e}") from e
        self._lines = queue.Queue()
        threading.Thread(target=self._read, args=(self._proc, self._lines), daemon=True).start()
        atexit.register(self.close)
        self._request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "wardscript", "version": "0.1.0b1"},
            },
        )
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    @staticmethod
    def _read(proc: subprocess.Popen[str], lines: queue.Queue[Any]) -> None:
        assert proc.stdout is not None
        try:
            for line in proc.stdout:
                line = line.strip()
                if line:
                    lines.put(line)
        except (OSError, ValueError):
            pass  # Closed by `close()`.
        lines.put(_EOF)

    def close(self) -> None:
        proc, self._proc = self._proc, None
        if proc is None:
            return
        try:
            if proc.stdin is not None:
                proc.stdin.close()
            proc.wait(timeout=2)
        except Exception:
            proc.kill()
            proc.wait()
        if proc.stdout is not None:
            proc.stdout.close()

    # -- JSON-RPC --

    def _send(self, message: dict) -> None:
        assert self._proc is not None and self._proc.stdin is not None
        try:
            self._proc.stdin.write(json.dumps(message) + "\n")
            self._proc.stdin.flush()
        except OSError as e:
            raise ToolError(f"MCP server `{self.name}` stopped: {e}") from e

    def _request(self, method: str, params: dict) -> Any:
        self._next_id += 1
        id_ = self._next_id
        self._send({"jsonrpc": "2.0", "id": id_, "method": method, "params": params})
        while True:
            try:
                line = self._lines.get(timeout=self.timeout)
            except queue.Empty:
                raise ToolError(
                    f"MCP server `{self.name}` didn't answer `{method}` within {self.timeout:g}s"
                ) from None
            if line is _EOF:
                raise ToolError(f"MCP server `{self.name}` exited during `{method}`")
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue  # Not protocol output.
            if "method" in message:
                if "id" in message:  # A request from the server, e.g. `ping`.
                    reply: dict[str, Any] = {"jsonrpc": "2.0", "id": message["id"]}
                    if message["method"] == "ping":
                        reply["result"] = {}
                    else:
                        reply["error"] = {"code": -32601, "message": "not supported"}
                    self._send(reply)
                continue
            if message.get("id") != id_:
                continue
            if "error" in message:
                err = message["error"]
                raise ToolError(f"MCP server `{self.name}`: `{method}` failed: {err.get('message', err)}")
            return message.get("result")

    # -- tools --

    def list_tools(self) -> list[dict]:
        with self._lock:
            self._start()
            tools: list[dict] = []
            cursor = None
            while True:
                result = self._request("tools/list", {"cursor": cursor} if cursor else {})
                tools.extend(result.get("tools", []))
                cursor = result.get("nextCursor")
                if not cursor:
                    return tools

    def call_tool(self, name: str, arguments: Mapping[str, Any]) -> Any:
        """Calls a tool with named arguments and returns its result (see the module)."""
        with self._lock:
            self._start()
            result = self._request("tools/call", {"name": name, "arguments": dict(arguments)})
        text = "\n".join(
            c.get("text", "") for c in result.get("content", []) if c.get("type") == "text"
        )
        if result.get("isError"):
            raise Thrown(text or f"`{name}` failed")
        if result.get("structuredContent") is not None:
            return result["structuredContent"]
        return text


def load_config(path: str | os.PathLike[str] = "mcp.json") -> dict[str, Server]:
    """The servers in an `mcp.json` (`{"mcpServers": {name: {command, args, env}}}`),
    keyed by name, for `configure(tools=...)`. Relative paths run from its directory."""
    path = os.fspath(path)
    with open(path, encoding="utf-8") as f:
        config = json.load(f)
    base = os.path.dirname(os.path.abspath(path))
    servers: dict[str, Server] = {}
    for name, spec in (config.get("mcpServers") or {}).items():
        if "command" not in spec:
            continue  # Only stdio servers are supported.
        servers[name] = Server(
            spec["command"], spec.get("args", ()), spec.get("env"), cwd=base, name=name
        )
    return servers


__all__ = ["PROTOCOL_VERSION", "Server", "load_config"]
