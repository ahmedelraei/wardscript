# Wardscript for VS Code

Highlighting for `.ward` and `.wardscript` files, and the Wardscript language server
(`ward lsp`): diagnostics as you type (with their `W0xxx` codes and the path
untrusted data took), the type of an expression on hover, go to definition, and
formatting (`ward fmt`).

## Install from source

```bash
cargo install --path crates/ward_cli     # puts `ward` on your PATH
cd editors/vscode
npm install
npx @vscode/vsce package                 # makes wardscript-0.0.1.vsix
code --install-extension wardscript-0.0.1.vsix
```

If `ward` isn't on your `PATH`, set `wardscript.path` in the settings to the
executable.
