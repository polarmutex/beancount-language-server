import * as vscode from "vscode";
import { log } from "./util";
import * as package_json from "../package.json";

export class Config {
  readonly extensionId = "polarmutex.beancountLangServer";

  private readonly rootSection = "beancountLangServer";
  private readonly requiresReloadOpts = ["serverPath", "journalFile"].map(
    (opt) => `${this.rootSection}.${opt}`,
  );

  constructor(ctx: vscode.ExtensionContext) {
    vscode.workspace.onDidChangeConfiguration(
      this.onDidChangeConfiguration,
      undefined,
      ctx.subscriptions,
    );
    this.refreshLogging();
  }

  private refreshLogging() {
    log.debug(
      "Extension version:",
      package_json.version,
      "using configuration:",
      this.cfg,
    );
  }

  private onDidChangeConfiguration = async (
    event: vscode.ConfigurationChangeEvent,
  ) => {
    this.refreshLogging();

    const requiresReloadOpt = this.requiresReloadOpts.find((opt) =>
      event.affectsConfiguration(opt),
    );

    if (!requiresReloadOpt) return;

    const userResponse = await vscode.window.showInformationMessage(
      `Changing "${requiresReloadOpt}" requires a reload`,
      "Reload now",
    );

    if (userResponse === "Reload now") {
      await vscode.commands.executeCommand("workbench.action.reloadWindow");
    }
  };

  private get cfg(): vscode.WorkspaceConfiguration {
    return vscode.workspace.getConfiguration(this.rootSection);
  }

  get serverPath() {
    return this.cfg.get<null | string>("serverPath");
  }
  get journalFile() {
    return this.cfg.get<null | string>("journalFile");
  }
}
