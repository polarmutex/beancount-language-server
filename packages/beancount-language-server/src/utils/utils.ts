import { Range } from "vscode-languageserver";

export class Utils {
    public static rangeEquals(a: Range, b: Range): boolean {
    return (
      a.start.character === b.start.character &&
      a.start.line === b.start.line &&
      a.end.character === b.end.character &&
      a.end.line === b.end.line
    );
  }

  public static arrayEquals<T>(
    a: T[],
    b: T[],
    itemEquals: (a: T, b: T) => boolean = (a, b): boolean => a === b,
  ): boolean {
    if (a === b) {
      return true;
    }
    if (a.length !== b.length) {
      return false;
    }
    return a.every((x, i) => itemEquals(x, b[i]));
  }

}
