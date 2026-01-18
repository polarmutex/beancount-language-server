use pyo3::prelude::*;

#[pyfunction]
fn main(args: Vec<String>) -> PyResult<i32> {
    Ok(beancount_language_server::main(args))
}

#[pymodule]
fn _beancount_lsp(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(main, module)?)?;
    Ok(())
}
