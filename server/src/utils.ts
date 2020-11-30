import { spawn, SpawnOptions } from 'child_process';

export function runExternalCommand(
    cmd: string,
    args: string[],
    callBack: (stdout: string) => void,
    opts?: SpawnOptions,
    logger?: (str: string) => void
) {

    const options: SpawnOptions = opts ? opts : {};
    const child = spawn(cmd, args, options);

    if (logger) {
        child.on('error', (e: string) => logger('error: ' + e));
        child.stderr!.on('date', e => logger('stderr: ' + e));
    }

    let response = '';

    child.stdout!.on('data', buffer => {
        response += buffer.toString()
    });
    child.stdout!.on('end', () => {
        callBack(response);
    });
}
