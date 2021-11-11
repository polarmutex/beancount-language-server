import * as vscode from "vscode";
import { Config } from "./config";
import { getServerOrDownload } from "./download";
import * as lsp from "vscode-languageclient/node";
import { SemanticTokensProvider, buildLegend } from "./semantic_tokens";
import { log } from './util';

const TAG = "v1.0.0";

let client: lsp.LanguageClient;

export async function activate(context: vscode.ExtensionContext) {
log.error('test 1 2 3');
  const config = new Config(context);

  const server_path = await getServerOrDownload(context, TAG);
  const server_executable: lsp.Executable = {
    command: server_path,
    args: ["--stdio"],
    options: {
      env: { RUST_LOG: "warn" },
    },
  };

  const server_options: lsp.ServerOptions = {
    run: server_executable,
    debug: server_executable,
  };

  const client_options: lsp.LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "beancount" }],
    synchronize: {
      // Notify the server about file changes to '.clientrc files contained in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher("**/.beancount"),
    },
    initializationOptions: {
      journal_file: config.journalFile,
    },
  };

  client = new lsp.LanguageClient(
    "beancountLangServer",
    "Beancount Language Server",
    server_options,
    client_options
  );

  // Start the client. This will also launch the server
  client.start();

  const legend = buildLegend();
  const tokenProvider = new SemanticTokensProvider(legend);
  await tokenProvider.ast.init();

  context.subscriptions.push(
    vscode.languages.registerDocumentSemanticTokensProvider(
      { language: "beancount" },
      tokenProvider,
      legend
    )
  );
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}
