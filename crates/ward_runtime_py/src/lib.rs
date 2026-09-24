//! `wardscript._core`: the Rust runtime core for the Python package. Its interface matches
//! `wardscript/_core_py.py`, the fallback used when this module isn't built.

use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use serde_json::Value;
use ward_runtime::budget::{self, Exceeded, Limits};
use ward_runtime::trace::{self, Event};

fn json(text: &str) -> PyResult<Value> {
    serde_json::from_str(text).map_err(|e| PyValueError::new_err(e.to_string()))
}

type Over = Option<(&'static str, f64, f64)>;

fn over(e: Option<Exceeded>) -> Over {
    e.map(|e| (e.resource, e.limit, e.used))
}

/// A running function's budget. Each `charge_*` returns `(resource, limit, used)` for the
/// first resource over its limit, or `None`.
#[pyclass(module = "wardscript._core")]
struct Budget(budget::Budget);

#[pymethods]
impl Budget {
    #[new]
    #[pyo3(signature = (function, tokens=None, calls=None, cost=None, time=None))]
    fn new(
        function: String,
        tokens: Option<f64>,
        calls: Option<f64>,
        cost: Option<f64>,
        time: Option<f64>,
    ) -> Self {
        Budget(budget::Budget::new(
            function,
            Limits {
                tokens,
                calls,
                cost,
                time,
            },
        ))
    }

    #[getter]
    fn function(&self) -> String {
        self.0.function.clone()
    }

    #[getter]
    fn used(&self) -> (f64, f64, f64) {
        let u = self.0.used;
        (u.tokens, u.calls, u.cost)
    }

    fn charge_call(&mut self) -> Over {
        over(self.0.charge_call())
    }

    fn charge_usage(&mut self, tokens: f64, cost: f64) -> Over {
        over(self.0.charge_usage(tokens, cost))
    }

    fn check_time(&self) -> Over {
        over(self.0.check_time())
    }
}

/// Records a run's events. `record` takes an event as JSON (`{"kind": ..., ...}`) and
/// returns the full record as a JSON line.
#[pyclass(module = "wardscript._core")]
struct Recorder(trace::Recorder);

#[pymethods]
impl Recorder {
    #[new]
    #[pyo3(signature = (dir=None))]
    fn new(dir: Option<std::path::PathBuf>) -> PyResult<Self> {
        trace::Recorder::new(dir.as_deref())
            .map(Recorder)
            .map_err(|e| PyOSError::new_err(e.to_string()))
    }

    #[getter]
    fn run(&self) -> String {
        self.0.run.clone()
    }

    #[getter]
    fn path(&self) -> Option<String> {
        self.0.path().map(|p| p.display().to_string())
    }

    fn record(&mut self, event: &str) -> PyResult<String> {
        let event: Event =
            serde_json::from_str(event).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let record = self
            .0
            .record(event)
            .map_err(|e| PyOSError::new_err(e.to_string()))?;
        serde_json::to_string(&record).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

/// The digest of a JSON value.
#[pyfunction]
fn digest(value: &str) -> PyResult<String> {
    Ok(trace::digest(&json(value)?))
}

/// `(path, digest)` of a JSON value and each of its parts.
#[pyfunction]
fn leaves(value: &str) -> PyResult<Vec<(String, String)>> {
    Ok(trace::leaves(&json(value)?)
        .into_iter()
        .map(|l| (l.path, l.digest))
        .collect())
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Budget>()?;
    m.add_class::<Recorder>()?;
    m.add_function(wrap_pyfunction!(digest, m)?)?;
    m.add_function(wrap_pyfunction!(leaves, m)?)?;
    m.add("IMPLEMENTATION", "rust")?;
    Ok(())
}
