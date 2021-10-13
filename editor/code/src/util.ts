export const log = new (class {
  private enabled = true;

  setEnabled(yes: boolean): void {
    log.enabled = yes;
  }

  debug(message?: any, ...optionalParams: any[]): void {
    if (!log.enabled) return;
    // eslint-disable-next-line no-console
    console.log(message, ...optionalParams);
  }

  error(message?: any, ...optionalParams: any[]): void {
    if (!log.enabled) return;
    debugger;
    // eslint-disable-next-line no-console
    console.error(message, ...optionalParams);
  }
})();
