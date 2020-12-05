import { spawn, SpawnOptions } from 'child_process';

export function runExternalCommand(
    cmd: string,
    args: string[],
    callBack: (stdout?: string) => void,
    opts?: SpawnOptions,
    logger?: (str: string) => void
) {

    const options: SpawnOptions = opts ? opts : {};
    const child = spawn(cmd, args, options);

    if (logger) {
        child.on('error', (e: string) => logger('error: ' + e));
        child.stderr!.on('data', e => logger('stderr: ' + e));
    }

    let response: string | undefined = undefined;

    child.stdout!.on('data', buffer => {
        if (response === undefined) {
            response = ""
        }
        response += buffer.toString()
    });
    child.stdout!.on('end', () => {
        callBack(response);
    });
}
