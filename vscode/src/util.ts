import { inspect } from "util";
import { window } from "vscode";

export type Logger = {
  info(...msg: [unknown, ...unknown[]]): void;
  debug(...msg: [unknown, ...unknown[]]): void;
  error(...msg: [unknown, ...unknown[]]): void;
};

const output = window.createOutputChannel(
  "beancount-language-server (extension)",
);

function stringify(val: unknown): string {
  if (typeof val === "string") return val;
  return inspect(val, {
    colors: false,
    depth: 6, // heuristic
  });
}

function write(label: string, ...messageParts: unknown[]): void {
  const message = messageParts.map((part) => stringify(part)).join(" ");
  const dateTime = new Date().toLocaleString();
  output.appendLine(`${label} [${dateTime}]: ${message}`);
}

export const log: Logger = {
  info: (...msg) => write("INFO", ...msg),
  debug: (...msg) => write("DEBUG", ...msg),
  error: (...msg) => {
    write("ERROR", ...msg);
    output.show(true);
  },
};
