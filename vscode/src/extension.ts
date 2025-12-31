import * as vscode from "vscode";
import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient;

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  const server_path = await get_server_path(context);
  if (!server_path) {
    await vscode.window.showErrorMessage(
      "The beancount-language-server extension doesn't ship with prebuilt binaries for your platform yet. " +
      "You can still use it by cloning the polarmutex/beancount-language-server repo from GitHub to build the LSP " +
      "yourself and use it with this extension with the beancountLangServer.serverPath setting"
    );
    return;
  }

  const server_executable: Executable = {
    command: server_path,
  };

  const server_options: ServerOptions = {
    run: server_executable,
    debug: server_executable,
  };

  const config = vscode.workspace.getConfiguration("beancountLangServer");
  const client_options: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "beancount" }],
    synchronize: {
      //  // Notify the server about file changes to '.clientrc files contained in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher("**/.{bean,beancount}"),
    },
    initializationOptions: {
      journal_file: config.get("journalFile"),
    },
  };

  client = new LanguageClient(
    "beancount-language-server",
    "Beancount Language Server",
    server_options,
    client_options
  );

  // Start the client. This will also launch the server
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

type Architecture = "x64" | "arm64";
type PlatformTriplets = {
  [P in NodeJS.Platform]?: {
    [A in Architecture]: string;
  };
};

const PLATFORM_TRIPLETS: PlatformTriplets = {
  win32: { x64: "x86_64-pc-windows-msvc", arm64: "aarch64-pc-windows-msvc" },
  darwin: { x64: "x86_64-apple-darwin", arm64: "aarch64-apple-darwin" },
  linux: {
    x64: "x86_64-unknown-linux-gnu",
    arm64: "aarch64-unknown-linux-gnu",
  },
};

async function get_server_path(
  context: vscode.ExtensionContext
): Promise<string | undefined> {
  const config = vscode.workspace.getConfiguration("beancountLangServer");
  const explicitPath = config.get("serverPath");
  if (typeof explicitPath === "string" && explicitPath !== "") {
    return explicitPath;
  }

  const triplet =
    PLATFORM_TRIPLETS[process.platform]?.[process.arch as Architecture];
  if (!triplet) {
    return undefined;
  }

  const binaryExt = triplet.includes("windows") ? ".exe" : "";
  const binaryName = `beancount-language-server${binaryExt}`;

  const bundlePath = vscode.Uri.joinPath(
    context.extensionUri,
    "server",
    binaryName
  );
  const bundleExists = await vscode.workspace.fs.stat(bundlePath).then(
    () => true,
    () => false
  );

  return bundleExists ? bundlePath.fsPath : undefined;
}
