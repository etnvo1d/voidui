#!/usr/bin/env python3
"""Check style completion, hover docs, and navigation against a real rust-analyzer.

Run from any directory with Python 3 and a matching rust-analyzer toolchain.
The isolated fixture, server log, and results are written under target/style-editor.
This check does not edit your IDE settings or require third-party Python packages.
"""
import argparse
import json
import pathlib
import queue
import shutil
import subprocess
import threading
import time

SOURCE = '''mod components {
    use ui::{Children, component, div};
    #[component]
    pub fn panel<const N: usize>(#[prop(default = N as f32)] padding: f32, children: Children) {
        div().padding(padding).children(children)
    }
}

pub fn widget() -> impl ui::IntoElement {
    ui::div()
        .width(64) // probe: widget_width
        .w_full() // probe: widget_full
}

pub fn closure_component() -> impl ui::IntoElement {
    ui::component(ui::div)
        .width(64) // probe: component_width
        .w_full() // probe: component_full
}

pub fn named_component() -> impl ui::IntoElement {
    components::panel::<8>()
        .padding(12.0) // probe: named_input
        .w_full() // probe: named_full
        .p_4() // probe: named_padding
        .border_r_1() // probe: named_border
        .cursor_col_resize() // probe: named_cursor
        .child("Content") // probe: named_child
        .build()
        .w_16() // probe: built_width
}
'''

# Each marker checks its own receiver type, including after preceding style calls.
CHECKS = [
    ("widget_width", "width", "Set CSS width"),
    ("widget_full", "w_full", "100%"),
    ("component_width", "width", "Set CSS width"),
    ("component_full", "w_full", "100%"),
    ("named_input", "padding", "component input"),
    ("named_full", "w_full", "100%"),
    ("named_padding", "p_4", "16px"),
    ("named_border", "border_r_1", "1px"),
    ("named_cursor", "cursor_col_resize", "col-resize"),
    ("named_child", "child", "Append a component"),
    ("built_width", "w_16", "64px"),
]
COMPLETIONS = {"widget_full", "component_full", "named_input", "named_full", "named_cursor", "named_child"}


class Client:
    """Minimal synchronous LSP client; continuously drain the server's pipe."""

    def __init__(self, executable, cwd, log, timeout):
        self.process = subprocess.Popen(
            [executable], cwd=cwd, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=log,
        )
        self.timeout = timeout
        self.sequence = 0
        self.messages = queue.Queue()
        self.responses = {}
        self.status = None
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        try:
            while True:
                headers = {}
                while True:
                    line = self.process.stdout.readline()
                    if not line:
                        return
                    if line in (b"\r\n", b"\n"):
                        break
                    key, value = line.decode().split(":", 1)
                    headers[key.lower()] = value.strip()
                body = self.process.stdout.read(int(headers["content-length"]))
                self.messages.put(json.loads(body))
        finally:
            self.messages.put(None)

    def send(self, message):
        raw = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(raw)}\r\n\r\n".encode() + raw)
        self.process.stdin.flush()

    def notify(self, method, params):
        self.send({"method": method, "params": params})

    def receive(self, deadline):
        try:
            message = self.messages.get(timeout=max(0.01, deadline - time.monotonic()))
        except queue.Empty as error:
            raise TimeoutError("rust-analyzer did not finish within the timeout; inspect server.log") from error
        if message is None:
            raise RuntimeError("rust-analyzer exited unexpectedly; inspect server.log")
        if "method" in message and "id" in message:
            self.send({"id": message["id"], "result": None})
        elif "id" in message:
            self.responses[message["id"]] = message
        elif message.get("method") == "experimental/serverStatus":
            self.status = message["params"]
        return message

    def request(self, method, params, timeout=None):
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        while time.monotonic() < deadline:
            self.sequence += 1
            request_id = self.sequence
            self.send({"id": request_id, "method": method, "params": params})
            while request_id not in self.responses:
                self.receive(deadline)
            response = self.responses.pop(request_id)
            if "error" not in response:
                return response.get("result")
            # LSP permits cancellation when workspace loading changes the analysis
            # snapshot. Retry that signal within the original deadline, never an
            # empty hover/completion or a semantic error that could hide a regression.
            if response["error"].get("code") != -32801:
                raise RuntimeError(f"{method}: {response['error']}")
        raise TimeoutError(f"{method}: workspace kept changing until the timeout")

    def ready(self):
        deadline = time.monotonic() + self.timeout
        while not self.status or not self.status.get("quiescent"):
            self.receive(deadline)
        if self.status.get("health") == "error":
            raise RuntimeError(f"rust-analyzer workspace loading failed: {self.status}")

    def close(self):
        try:
            if self.process.poll() is None:
                self.request("shutdown", None, timeout=5)
                self.notify("exit", None)
                self.process.wait(timeout=5)
        except (BrokenPipeError, RuntimeError, TimeoutError, subprocess.TimeoutExpired):
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()


def location(marker, method, prefix_length):
    for line, source in enumerate(SOURCE.splitlines()):
        if source.rstrip().endswith(f"// probe: {marker}"):
            column = source.index(f".{method}(") + 1 + prefix_length
            # LSP positions count UTF-16 code units, including in non-ASCII paths/code.
            return {"line": line, "character": len(source[:column].encode("utf-16-le")) // 2}
    raise AssertionError(f"missing fixture marker: {marker}")


def run(executable, timeout):
    project = pathlib.Path(__file__).resolve().parents[1]
    fixture = project / "target" / "style-editor"
    (fixture / "src").mkdir(parents=True, exist_ok=True)
    manifest = '''[package]
name = "voidui_style_editor_check"
version = "0.0.0"
edition = "2024"
[dependencies]
ui = { package = "voidui", path = "../..", default-features = false }
[workspace]
'''
    (fixture / "Cargo.toml").write_text(manifest)
    source = fixture / "src" / "lib.rs"
    source.write_text(SOURCE)
    version = subprocess.check_output([executable, "--version"], text=True).strip()
    results = {"server": version, "checks": {}}
    print(version, flush=True)
    with (fixture / "server.log").open("w") as log:
        client = Client(executable, fixture, log, timeout)
        try:
            client.request("initialize", {
                "processId": None,
                "rootUri": fixture.as_uri(),
                "workspaceFolders": [{"uri": fixture.as_uri(), "name": "style-editor"}],
                "capabilities": {
                    "experimental": {"serverStatusNotification": True},
                    "textDocument": {
                        "hover": {"contentFormat": ["markdown"]},
                        "completion": {"completionItem": {"snippetSupport": False}},
                    },
                },
                "initializationOptions": {
                    "procMacro": {"enable": True},
                    "cargo": {"buildScripts": {"enable": True}, "targetDir": str(project / "target")},
                    "cachePriming": {"enable": False},
                    "checkOnSave": False,
                    "numThreads": 4,
                },
            })
            client.notify("initialized", {})
            client.notify("textDocument/didOpen", {"textDocument": {
                "uri": source.as_uri(), "languageId": "rust", "version": 1, "text": SOURCE,
            }})
            client.ready()
            for marker, method, expected_doc in CHECKS:
                params = {"textDocument": {"uri": source.as_uri()}, "position": location(marker, method, 1)}
                hover = client.request("textDocument/hover", params)
                entry = results["checks"][marker] = {"hover": hover}
                assert hover and expected_doc in json.dumps(hover), f"{marker}: missing method or docs"
                definition = client.request("textDocument/definition", params)
                entry["definition"] = definition
                assert definition, f"{marker}: cannot navigate to method definition"
                if marker in COMPLETIONS:
                    params["position"] = location(marker, method, min(2, len(method)))
                    completion = client.request("textDocument/completion", params)
                    items = completion.get("items", []) if isinstance(completion, dict) else (completion or [])
                    labels = [item.get("label", "") for item in items]
                    matching = [label for label in labels if label == method or label.startswith(method + "(")]
                    entry["completion"] = matching
                    assert matching, f"{marker}: method not offered in completion"
                print(f"PASS {marker}: {method}", flush=True)
        finally:
            (fixture / "results.json").write_text(json.dumps(results, indent=2))
            client.close()
    print(f"Verified {len(CHECKS)} hovers/definitions and {len(COMPLETIONS)} completions. Results: {fixture / 'results.json'}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-analyzer", default="rust-analyzer", help="LSP server executable")
    parser.add_argument("--timeout", type=float, default=240, help="Maximum seconds per load/request")
    args = parser.parse_args()
    executable = shutil.which(args.rust_analyzer)
    if executable is None:
        parser.error("rust-analyzer was not found; install it for the active Rust toolchain")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    run(executable, args.timeout)
