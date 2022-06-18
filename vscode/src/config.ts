import * as vscode from "vscode";
import { log } from "./util";

export class Config {
  readonly extensionId = "polarmutex.beancountLangServer";

  private readonly rootSection = "beancountLangServer";
  private readonly requiresReloadOpts = ["serverPath", "journalFile"].map(
    (opt) => `${this.rootSection}.${opt}`
  );

  readonly package: {
    version: string;
    releaseTag: string | null;
  } = vscode.extensions.getExtension(this.extensionId)!.packageJSON;

  constructor(ctx: vscode.ExtensionContext) {
    vscode.workspace.onDidChangeConfiguration(
      this.onDidChangeConfiguration,
      this,
      ctx.subscriptions
    );
    this.refreshLogging();
  }

  private refreshLogging() {
    log.setEnabled(this.traceExtension);
    log.debug(
      "Extension version:",
      this.package.version,
      "using configuration:",
      this.cfg
    );
  }

  private async onDidChangeConfiguration(
    event: vscode.ConfigurationChangeEvent
  ) {
    this.refreshLogging();

    const requiresReloadOpt = this.requiresReloadOpts.find((opt) =>
      event.affectsConfiguration(opt)
    );

    if (!requiresReloadOpt) return;

    const userResponse = await vscode.window.showInformationMessage(
      `Changing "${requiresReloadOpt}" requires a reload`,
      "Reload now"
    );

    if (userResponse === "Reload now") {
      await vscode.commands.executeCommand("workbench.action.reloadWindow");
    }
  }

  private get cfg(): vscode.WorkspaceConfiguration {
    return vscode.workspace.getConfiguration(this.rootSection);
  }

  get traceExtension() {
    return this.cfg.get<boolean>("trace.extension")!;
  }
  get serverPath() {
    return this.cfg.get<null | string>("serverPath")!;
  }
  get journalFile() {
    return this.cfg.get<null | string>("journalFile")!;
  }
}
