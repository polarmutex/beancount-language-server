import * as fs from "fs";
import * as path from "path";
import { inspect } from "util";
import * as vscode from "vscode";
import {
  CloseAction,
  CloseHandlerResult,
  ErrorAction,
  ErrorHandlerResult,
  Executable,
  LanguageClient,
  LanguageClientOptions,
  Message,
  ServerOptions,
} from "vscode-languageclient/node";

import { log } from "./util";

let client: LanguageClient | undefined;

const lspOutputChannel = vscode.window.createOutputChannel(
  "beancount-language-server (server)",
);

async function start_or_restart_client(
  context: vscode.ExtensionContext,
  showRestartMessage: boolean,
): Promise<void> {
  const serverSelection = await get_server_path(context);
  if (!serverSelection) {
    await vscode.window.showErrorMessage(
      "The beancount-language-server extension doesn't ship with prebuilt binaries for your platform yet. " +
        "You can still use it by cloning the polarmutex/beancount-language-server repo from GitHub to build the LSP " +
        "yourself and use it with this extension with the beancountLangServer.serverPath setting",
    );
    return;
  }

  log.info("use lsp executable", {
    path: serverSelection.path,
    reason: serverSelection.reason,
  });

  const config = vscode.workspace.getConfiguration("beancountLangServer");

  const serverArgs: string[] = [];

  const server_executable: Executable = {
    command: serverSelection.path,
    args: serverArgs,
    options: {
      env: {
        RUST_BACKTRACE: "full",
      },
    },
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
    outputChannel: lspOutputChannel,
    documentSelector: [{ scheme: "file", language: "beancount" }],
    synchronize: {
      // Notify the server about file changes to beancount files contained in the workspace
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/.{bean,beancount}",
      ),
    },
    initializationOptions,
    errorHandler: {
      error(
        error: Error,
        message: Message | undefined,
        count: number | undefined,
      ): ErrorHandlerResult {
        log.error("client error: ", error, message, count);
        return {
          action: ErrorAction.Continue,
          message: inspect(message),
          handled: true,
        };
      },
      closed(): CloseHandlerResult {
        log.error("server stopped, restarting");
        vscode.window.showErrorMessage(
          "beancount-language-server stopped unexpectedly, restarting",
        );

        return {
          action: CloseAction.Restart,
          message: "server exit, restarting",
          handled: true,
        };
      },
    },
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

const SERVER_BINARY_NAME =
  process.platform === "win32"
    ? "beancount-language-server.exe"
    : "beancount-language-server";

type ServerSelection = {
  path: string;
  reason: string;
};

async function get_server_path(
  context: vscode.ExtensionContext,
): Promise<ServerSelection | undefined> {
  const resolvers: Array<{
    reason: string;
    resolver: () => Promise<string | undefined>;
  }> = [
    {
      reason: "config: beancountLangServer.serverPath",
      resolver: () => Promise.resolve(find_explicit_server_path()),
    },
    {
      reason: "workspace: .venv",
      resolver: () => find_in_workspace_venv(),
    },
    {
      reason: "PATH",
      resolver: () => find_on_path(),
    },
    {
      reason: "bundled binary",
      resolver: () => find_bundled_server_path(context),
    },
  ];

  for (const entry of resolvers) {
    const path = await entry.resolver();
    if (path) {
      return { path, reason: entry.reason };
    }
  }

  return undefined;
}

function find_explicit_server_path(): string | undefined {
  const config = vscode.workspace.getConfiguration("beancountLangServer");
  const explicitPath = config.get("serverPath");
  if (typeof explicitPath === "string" && explicitPath !== "") {
    return explicitPath;
  }
  return undefined;
}

async function find_bundled_server_path(
  context: vscode.ExtensionContext,
): Promise<string | undefined> {
  const triplet =
    PLATFORM_TRIPLETS[process.platform]?.[process.arch as Architecture];
  if (!triplet) {
    return undefined;
  }

  const bundlePath = vscode.Uri.joinPath(
    context.extensionUri,
    "server",
    triplet,
    SERVER_BINARY_NAME,
  );
  const bundleExists = await vscode.workspace.fs.stat(bundlePath).then(
    () => true,
    () => false,
  );

  return bundleExists ? bundlePath.fsPath : undefined;
}

async function find_in_workspace_venv(): Promise<string | undefined> {
  const folders = vscode.workspace.workspaceFolders ?? [];
  const venvNames = [".venv"];

  for (const folder of folders) {
    for (const venvName of venvNames) {
      const venvPath = path.join(folder.uri.fsPath, venvName);
      const binDir = venv_bin_dir(venvPath);
      if (!binDir) {
        continue;
      }
      const candidate = await find_in_dir(binDir);
      if (candidate) {
        return candidate;
      }
    }
  }

  return undefined;
}

function venv_bin_dir(venvPath: string): string | null {
  if (process.platform === "win32") {
    return path.join(venvPath, "Scripts");
  }
  return path.join(venvPath, "bin");
}

async function find_in_dir(dirPath: string): Promise<string | undefined> {
  const candidate = path.join(dirPath, SERVER_BINARY_NAME);
  try {
    await fs.promises.access(candidate, fs.constants.X_OK);
    return candidate;
  } catch (_) {
    return undefined;
  }
}

async function find_on_path(): Promise<string | undefined> {
  const candidates = process.env.PATH?.split(path.delimiter) ?? [];
  for (const dir of candidates) {
    const found = await find_in_dir(dir);
    if (found) {
      return found;
    }
  }
  return undefined;
}
