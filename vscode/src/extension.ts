import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

import { log } from "./util";

let client: LanguageClient | undefined;

async function start_or_restart_client(
  context: vscode.ExtensionContext,
  showRestartMessage: boolean,
): Promise<void> {
  const server_path = await get_server_path(context);
  if (!server_path) {
    await vscode.window.showErrorMessage(
      "The beancount-language-server extension doesn't ship with prebuilt binaries for your platform yet. " +
        "You can still use it by cloning the polarmutex/beancount-language-server repo from GitHub to build the LSP " +
        "yourself and use it with this extension with the beancountLangServer.serverPath setting",
    );
    return;
  }

  log.info("use lsp executable", server_path);

  const config = vscode.workspace.getConfiguration("beancountLangServer");

  const serverArgs: string[] = [];

  const server_executable: Executable = {
    command: server_path,
    args: serverArgs,
  };

  const server_options: ServerOptions = {
    run: server_executable,
    debug: server_executable,
  };

  type InitializationOptions = {
    journal_file?: string;
    formatting?: unknown;
    bean_check?: unknown;
  };

  const initializationOptions: InitializationOptions = {
    formatting: config.get("formatting"),
    bean_check: config.get("beanCheck"),
  };

  const journalFile = config.get<string>("journalFile");
  if (journalFile && journalFile.trim() !== "") {
    initializationOptions.journal_file = journalFile;
  }

  const client_options: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "beancount" }],
    synchronize: {
      // Notify the server about file changes to beancount files contained in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/.{bean,beancount}",
      ),
    },
    initializationOptions,
  };

  log.info(JSON.stringify(initializationOptions, null, 2));

  const next = new LanguageClient(
    "beancount-language-server",
    "Beancount Language Server",
    server_options,
    client_options,
  );

  if (client?.isRunning()) {
    await client.stop();
  }

  client = next;
  await client.start();

  if (showRestartMessage) {
    void vscode.window.showInformationMessage(
      "Beancount language server restarted with latest configuration.",
    );
  }
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "beancountLangServer.reloadServer",
      async () => {
        try {
          await start_or_restart_client(context, true);
        } catch (error) {
          const message =
            error instanceof Error ? error.message : String(error);
          void vscode.window.showErrorMessage(
            `Failed to restart language server: ${message}`,
          );
        }
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "beancountLangServer.startServer",
      async () => {
        try {
          if (client?.isRunning()) {
            void vscode.window.showInformationMessage(
              "Beancount language server is already running.",
            );
            return;
          }
          await start_or_restart_client(context, false);
          void vscode.window.showInformationMessage(
            "Beancount language server started.",
          );
        } catch (error) {
          const message =
            error instanceof Error ? error.message : String(error);
          void vscode.window.showErrorMessage(
            `Failed to start language server: ${message}`,
          );
        }
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "beancountLangServer.stopServer",
      async () => {
        try {
          if (!client?.isRunning()) {
            void vscode.window.showInformationMessage(
              "Beancount language server is not running.",
            );
            return;
          }
          await client.stop();
          void vscode.window.showInformationMessage(
            "Beancount language server stopped.",
          );
        } catch (error) {
          const message =
            error instanceof Error ? error.message : String(error);
          void vscode.window.showErrorMessage(
            `Failed to stop language server: ${message}`,
          );
        }
      },
    ),
  );

  await start_or_restart_client(context, false);
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
  context: vscode.ExtensionContext,
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
    triplet,
    binaryName,
  );
  const bundleExists = await vscode.workspace.fs.stat(bundlePath).then(
    () => true,
    () => false,
  );

  if (bundleExists) {
    return bundlePath.fsPath;
  }

  const onPath = await find_on_path(binaryName);
  return onPath ?? undefined;
}

async function find_on_path(binaryName: string): Promise<string | null> {
  const candidates = process.env.PATH?.split(path.delimiter) ?? [];
  const names =
    process.platform === "win32"
      ? [binaryName, `${binaryName}.exe`]
      : [binaryName];

  for (const dir of candidates) {
    for (const name of names) {
      const full = path.join(dir, name);
      try {
        await fs.promises.access(full, fs.constants.X_OK);
        return full;
      } catch (_) {
        // continue searching
      }
    }
  }
  return null;
}
