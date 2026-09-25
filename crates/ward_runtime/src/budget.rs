//! Budget counters. One `Budget` per call of a function with a `budget {...}` clause; the
//! host keeps the stack of active ones and charges each of them.

use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    pub tokens: Option<f64>,
    pub calls: Option<f64>,
    pub cost: Option<f64>,
    pub time: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub tokens: f64,
    pub calls: f64,
    pub cost: f64,
}

/// A resource that went over its limit.
#[derive(Clone, Debug, PartialEq)]
pub struct Exceeded {
    pub function: String,
    pub resource: &'static str,
    pub limit: f64,
    pub used: f64,
}

#[derive(Debug)]
pub struct Budget {
    pub function: String,
    pub limits: Limits,
    pub used: Usage,
    started: Instant,
}

impl Budget {
    pub fn new(function: impl Into<String>, limits: Limits) -> Self {
        Budget {
            function: function.into(),
            limits,
            used: Usage::default(),
            started: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn over(&self, resource: &'static str, limit: Option<f64>, used: f64) -> Option<Exceeded> {
        let limit = limit?;
        (used > limit).then(|| Exceeded {
            function: self.function.clone(),
            resource,
            limit,
            used,
        })
    }

    /// The first resource over its limit, if any.
    pub fn check(&self) -> Option<Exceeded> {
        let l = self.limits;
        self.over("tokens", l.tokens, self.used.tokens)
            .or_else(|| self.over("calls", l.calls, self.used.calls))
            .or_else(|| self.over("cost", l.cost, self.used.cost))
            .or_else(|| self.over("time", l.time, self.elapsed()))
    }

    /// Counts a model request about to be sent; refused if that goes over `calls`.
    pub fn charge_call(&mut self) -> Option<Exceeded> {
        self.used.calls += 1.0;
        self.check()
    }

    /// Whether this budget limits `cost`, so an answer of unknown cost can't be counted.
    pub fn limits_cost(&self) -> bool {
        self.limits.cost.is_some()
    }

    /// Charges an answer's usage. `cost` is `None` when unknown (a model without
    /// prices); it counts as 0, and the host decides from `unenforceable` whether that
    /// may go on.
    pub fn charge_usage(&mut self, tokens: f64, cost: Option<f64>) -> Option<Exceeded> {
        self.used.tokens += tokens;
        self.used.cost += cost.unwrap_or(0.0);
        self.check()
    }

    /// Whether an answer of this cost leaves the `cost` limit unenforceable.
    pub fn unenforceable(&self, cost: Option<f64>) -> bool {
        cost.is_none() && self.limits_cost()
    }

    pub fn check_time(&self) -> Option<Exceeded> {
        self.over("time", self.limits.time, self.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_tokens_and_cost() {
        let mut b = Budget::new(
            "f",
            Limits {
                calls: Some(2.0),
                tokens: Some(100.0),
                ..Limits::default()
            },
        );
        assert_eq!(b.charge_call(), None);
        assert_eq!(b.charge_usage(60.0, Some(0.0)), None);
        assert_eq!(b.charge_call(), None);
        let e = b.charge_usage(60.0, Some(0.0));
        assert_eq!(e.map(|e| (e.resource, e.used)), Some(("tokens", 120.0)));
        let e = b.charge_call();
        assert_eq!(e.map(|e| e.resource), Some("tokens"));
    }

    #[test]
    fn unknown_cost() {
        let mut priced = Budget::new(
            "f",
            Limits {
                cost: Some(0.01),
                ..Limits::default()
            },
        );
        assert!(priced.limits_cost());
        assert!(priced.unenforceable(None));
        assert!(!priced.unenforceable(Some(0.0)));
        assert_eq!(priced.charge_usage(10.0, None), None);
        let e = priced.charge_usage(10.0, Some(0.02));
        assert_eq!(e.map(|e| e.resource), Some("cost"));
        let other = Budget::new("g", Limits::default());
        assert!(!other.unenforceable(None));
    }

    #[test]
    fn time() {
        let b = Budget::new(
            "f",
            Limits {
                time: Some(0.0),
                ..Limits::default()
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_eq!(b.check_time().map(|e| e.resource), Some("time"));
    }
}
