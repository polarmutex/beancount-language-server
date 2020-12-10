import { spawn, SpawnOptions } from 'child_process';

export function runExternalCommand(
    cmd: string,
    args: string[],
    opts?: SpawnOptions,
    logger?: (str: string) => void
): Promise<string> {

    const options: SpawnOptions = opts ? opts : {};
    const child = spawn(cmd, args, options);

    return new Promise((resolve, reject) => {
        let response: string = '';

        const handleClose = (returnCode: number | Error) => {
            if (response != '') {
                resolve(response);
            }
            else {
                reject('Failed to execute bean-check')
            }
        }

        if (logger) {
            child.on('error', (e: string) => logger('error: ' + e));
            child.stderr!.on('data', e => logger('stderr: ' + e));
        }

        child.stdout!.on('data', buffer => {
            response += buffer.toString()
        });
        child.stdout!.on('end', handleClose);
    });
}
