import * as vscode from "vscode";
import * as os from "os";
import { Config } from "./config";

import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
} from "vscode-languageclient/node";

import { SemanticTokensProvider, buildLegend } from "./semantic_tokens";

let client: LanguageClient;

export async function activate(context: vscode.ExtensionContext) {
  const config = new Config(context);
  let server_path = config.serverPath;
  if (server_path.startsWith("~/")) {
    server_path = os.homedir() + server_path.slice("~".length);
  }
  const server_options: Executable = {
    command: server_path,
  };

  const client_options: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "beancount" }],
    synchronize: {
      // Notify the server about file changes to '.clientrc files contained in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher("**/.beancount"),
    },
    initializationOptions: {
      journal_file: config.journalFile,
    },
  };

  client = new LanguageClient(
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

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
