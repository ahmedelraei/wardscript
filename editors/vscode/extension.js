// Starts `ward lsp` for `.ward` files. The server does the work: diagnostics as you
// type, the type of an expression on hover, and go to definition.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const command = vscode.workspace.getConfiguration("wardscript").get("path") || "ward";
  const server = { command, args: ["lsp"] };
  client = new LanguageClient(
    "wardscript",
    "Wardscript",
    { run: server, debug: server },
    {
      documentSelector: [{ scheme: "file", language: "ward" }],
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher("**/{*.ward,*.wardscript,ward.lock}"),
      },
    },
  );
  context.subscriptions.push(client);
  client.start().catch((err) => {
    vscode.window.showErrorMessage(
      `Wardscript: couldn't start \`${command} lsp\` (${err.message}). Set "wardscript.path" to the ward executable.`,
    );
  });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
