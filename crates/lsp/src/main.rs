fn main() {
    let exit_code = beancount_language_server::main(std::env::args());
    std::process::exit(exit_code);
}
