import {
  CancellationToken,
  Connection,
  Diagnostic,
  DiagnosticSeverity,
  FileChangeType,
} from "vscode-languageserver";
import { Utils } from '../../utils/utils'

export const enum DiagnosticKind {
  BeanCheck,
  TreeSitter,
}

export interface BeancountDiagnostic extends Diagnostic {
    data: {
        uri: string;
    }
}


export function diagnosticsEquals(a: BeancountDiagnostic, b: BeancountDiagnostic): boolean {
  if (a === b) {
    return true;
  }

  return (
    a.message === b.message &&
    a.severity === b.severity &&
    a.source === b.source &&
    a.data.uri === b.data.uri &&
    Utils.rangeEquals(a.range, b.range) &&
    Utils.arrayEquals(
      a.relatedInformation ?? [],
      b.relatedInformation ?? [],
      (a, b) => {
        return (
          a.message === b.message &&
          Utils.rangeEquals(a.location.range, b.location.range) &&
          a.location.uri === b.location.uri
        );
      },
    ) &&
    Utils.arrayEquals(a.tags ?? [], b.tags ?? [])
  );
}

export class BeancountDiagnostics {
  private diagnostics: Map<DiagnosticKind, BeancountDiagnostic[]> = new Map<
    DiagnosticKind,
    BeancountDiagnostic[]
  >();

  constructor(public uri: string) {}

  public get(): Diagnostic[] {
    return [
      ...this.getForKind(DiagnosticKind.BeanCheck),
      ...this.getForKind(DiagnosticKind.TreeSitter),
    ];
  }

  public update(kind: DiagnosticKind, diagnostics: BeancountDiagnostic[]): boolean {
    const existing = this.getForKind(kind);
    if (Utils.arrayEquals(existing, diagnostics, diagnosticsEquals)) {
      return false;
    }

    this.diagnostics.set(kind, diagnostics);
    return true;
  }

  public getForKind(kind: DiagnosticKind): BeancountDiagnostic[] {
    return this.diagnostics.get(kind) ?? [];
  }
}
