import { exec } from "child_process";
import * as vscode from "vscode";
import {
  Executable,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

import { PersistentState } from "./persistent_state";

let client: LanguageClient;

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  const state = new PersistentState(context.globalState);
  //TODO needeed? if (server_path.startsWith("~/")) {
  //  server_path = os.homedir() + server_path.slice("~".length);
  //}
  const server_path = await get_server_path(context, state);
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
    //synchronize: {
    //  // Notify the server about file changes to '.clientrc files contained in the workspace
    //  fileEvents: vscode.workspace.createFileSystemWatcher("**/.beancount"),
    //},
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

async function isNixOs(): Promise<boolean> {
  try {
    const contents = (
      await vscode.workspace.fs.readFile(vscode.Uri.file("/etc/os-release"))
    ).toString();
    const idString =
      contents.split("\n").find((a) => a.startsWith("ID=")) || "ID=linux";
    return idString.indexOf("nixos") !== -1;
  } catch {
    return false;
  }
}

async function patchelf(dest: vscode.Uri): Promise<void> {
  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Patching beancount-language-server for NixOS",
    },
    async (progress, _) => {
      const expression = `
            {srcStr, pkgs ? import <nixpkgs> {}}:
                pkgs.stdenv.mkDerivation {
                    name = "beancount-language-server";
                    src = /. + srcStr;
                    phases = [ "installPhase" "fixupPhase" ];
                    installPhase = "cp $src $out";
                    fixupPhase = ''
                    chmod 755 $out
                    patchelf --set-interpreter "$(cat $NIX_CC/nix-support/dynamic-linker)" $out
                    '';
                }
            `;
      const origFile = vscode.Uri.file(dest.fsPath + "-orig");
      await vscode.workspace.fs.rename(dest, origFile, { overwrite: true });
      try {
        progress.report({ message: "Patching executable", increment: 20 });
        await new Promise((resolve, reject) => {
          const handle = exec(
            `nix-build -E - --argstr srcStr '${origFile.fsPath}' -o '${dest.fsPath}'`,
            (err, stdout, stderr) => {
              if (err != null) {
                reject(Error(stderr));
              } else {
                resolve(stdout);
              }
            }
          );
          handle.stdin?.write(expression);
          handle.stdin?.end();
        });
      } finally {
        await vscode.workspace.fs.delete(origFile);
      }
    }
  );
}

async function get_server_path(
  context: vscode.ExtensionContext,
  state: PersistentState
): Promise<string | undefined> {
  const config = vscode.workspace.getConfiguration("beancountLangServer");
  const explicitPath = config.get("serverPath");
  if (typeof explicitPath === "string" && explicitPath !== "") {
    return explicitPath;
  }

  const triplet = PLATFORM_TRIPLETS[process.platform]?.[process.arch];
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

  //if (bundleExists) {
  //  let server = bundlePath;
  //  if (await isNixOs()) {
  //    await vscode.workspace.fs.createDirectory(config.globalStorageUri).then();
  //    const dest = vscode.Uri.joinPath(config.globalStorageUri, binaryName);
  //    let exists = await vscode.workspace.fs.stat(dest).then(
  //      () => true,
  //      () => false
  //    );
  //    if (exists && config.package.version !== state.serverVersion) {
  //      await vscode.workspace.fs.delete(dest);
  //      exists = false;
  //    }
  //    if (!exists) {
  //      await vscode.workspace.fs.copy(bundlePath, dest);
  //      await patchelf(dest);
  //    }
  //    server = dest;
  //  }
  //await state.updateServerVersion(config.package.version);
  //return server.fsPath;
  //}

  return bundleExists ? bundlePath.fsPath : undefined;
}
