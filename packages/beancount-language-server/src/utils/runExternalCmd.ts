import { Connection } from "vscode-languageserver";
import execa, { ExecaSyncReturnValue } from "execa";

export function execCmdSync(
  cmd: string,
  cmdArguments: string[],
  connection: Connection,
): ExecaSyncReturnValue<string> {

    try {

        return execa.sync(cmd, cmdArguments, {});

    } catch (error) {

        connection.console.warn(JSON.stringify(error));
        if (error.errno && error.errno === "ENOENT") {
            connection.window.showErrorMessage(
                `Cannot find executable with name '${cmd}'`,
            );
            throw "Executable not found";
        } else {
            throw error;
        }
    }
}
