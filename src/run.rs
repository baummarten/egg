use std::fmt;

use indexmap::IndexMap;
use instant::Instant;
use log::*;

use crate::{EGraph, Id, Language, Metadata, RecExpr, Rewrite, SearchMatches};

/// Data generated by running a [`Runner`] one iteration.
///
/// If the `serde-1` feature is enabled, this implements
/// [`serde::Serialize`][ser], which is useful if you want to output
/// this as a JSON or some other format.
///
/// [`Runner`]: trait.Runner.html
/// [ser]: https://docs.rs/serde/latest/serde/trait.Serialize.html
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-1", derive(serde::Serialize))]
#[non_exhaustive]
pub struct Iteration {
    /// The number of enodes in the egraph at the start of this
    /// iteration.
    pub egraph_nodes: usize,
    /// The number of eclasses in the egraph at the start of this
    /// iteration.
    pub egraph_classes: usize,
    /// A map from rule name to number of times it was _newly_ applied
    /// in this iteration.
    pub applied: IndexMap<String, usize>,
    /// Seconds spent searching in this iteration.
    pub search_time: f64,
    /// Seconds spent applying rules in this iteration.
    pub apply_time: f64,
    /// Seconds spent [`rebuild`](struct.EGraph.html#method.rebuild)ing
    /// the egraph in this iteration.
    pub rebuild_time: f64,
    // TODO optionally put best cost back in there
    // pub best_cost: Cost,
}

/// Data generated by running a [`Runner`] to completion.
///
/// If the `serde-1` feature is enabled, this implements
/// [`serde::Serialize`][ser], which is useful if you want to output
/// this as a JSON or some other format.
///
/// [`Runner`]: trait.Runner.html
/// [ser]: https://docs.rs/serde/latest/serde/trait.Serialize.html
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "serde-1",
    derive(serde::Serialize),
    serde(bound(serialize = "
        L: Language + std::fmt::Display,
        E: serde::Serialize
    "))
)]
#[non_exhaustive]
pub struct RunReport<L, E> {
    /// The initial expression added to the egraph.
    pub initial_expr: RecExpr<L>,
    /// The eclass id of the initial expression added to the egraph.
    pub initial_expr_eclass: Id,
    // pub initial_cost: Cost,
    /// The data generated by each [`Iteration`](struct.Iteration.html).
    pub iterations: Vec<Iteration>,
    // pub final_expr: RecExpr<L>,
    // pub final_cost: Cost,
    /// The total time spent running rules
    pub rules_time: f64,
    // pub extract_time: f64,
    /// The reason the [`Runner`](trait.Runner.html) stop iterating.
    pub stop_reason: E,
    // metrics
    // pub ast_size: usize,
    // pub ast_depth: usize,
}

/** Faciliates running rewrites over an [`EGraph`].

One use for [`EGraph`]s is as the basis of a rewriting system.
Since an egraph never "forgets" state when applying a [`Rewrite`], you
can apply many rewrites many times quite efficiently.
After the egraph is "full" (the rewrites can no longer find new
equalities) or some other condition, the egraph compactly represents
many, many equivalent expressions.
At this point, the egraph is ready for extraction (see [`Extractor`])
which can pick the represented expression that's best according to
some cost function.

This technique is called
[equality saturation](https://www.cs.cornell.edu/~ross/publications/eqsat/)
in general.
However, there can be many challenges in implementing this "outer
loop" of applying rewrites, mostly revolving around which rules to run
and when to stop.

Implementing the [`Runner`] trait allows you to customize this outer
loop in many ways.
Many of [`Runner`]s method have default implementation, and these call
the various hooks ([`pre_step`], [`during_step`], [`post_step`])
during their operation.

[`SimpleRunner`] is `egg`'s provided [`Runner`] that has reasonable
defaults and implements many useful things like saturation checking,
an egraph size limits, and rule back off.
Consider using [`SimpleRunner`] before implementing your own
[`Runner`].

[`EGraph`]: struct.EGraph.html
[`Extractor`]: struct.Extractor.html
[`SimpleRunner`]: struct.SimpleRunner.html
[`Runner`]: trait.Runner.html
[`pre_step`]: trait.Runner.html#method.pre_step
[`during_step`]: trait.Runner.html#method.during_step
[`post_step`]: trait.Runner.html#method.post_step
*/
pub trait Runner<L, M>
where
    L: Language,
    M: Metadata<L>,
{
    /// The type of an error that should stop the runner.
    ///
    /// This will be recorded in
    /// [`RunReport`](struct.RunReport.html#structfield.stop_reason).
    type Error: fmt::Debug;
    // TODO make it so Runners can add fields to Iteration data

    /// The pre-iteration hook. If this returns an error, then the
    /// search will stop. Useful for checking stop conditions or
    /// updating `Runner` state.
    ///
    /// Default implementation simply returns `Ok(())`.
    fn pre_step(&mut self, _egraph: &mut EGraph<L, M>) -> Result<(), Self::Error> {
        Ok(())
    }

    /// The post-iteration hook. If this returns an error, then the
    /// search will stop. Useful for checking stop conditions or
    /// updating `Runner` state.
    ///
    /// Default implementation simply returns `Ok(())`.
    fn post_step(
        &mut self,
        _iteration: &Iteration,
        _egraph: &mut EGraph<L, M>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// The intra-iteration hook. If this returns an error, then the
    /// search will stop. Useful for checking stop conditions.
    /// This is called after search each rule and after applying each rule.
    ///
    /// Default implementation simply returns `Ok(())`.
    fn during_step(&mut self, _egraph: &EGraph<L, M>) -> Result<(), Self::Error> {
        Ok(())
    }

    /// A hook allowing you to customize rewrite search behavior.
    /// Useful to implement rule management.
    ///
    /// Default implementation just calls
    /// [`Rewrite::search`](struct.Rewrite.html#method.search).
    fn search_rewrite(
        &mut self,
        egraph: &mut EGraph<L, M>,
        rewrite: &Rewrite<L, M>,
    ) -> Vec<SearchMatches> {
        rewrite.search(egraph)
    }

    /// A hook allowing you to customize rewrite application behavior.
    /// Useful to implement rule management.
    ///
    /// Default implementation just calls
    /// [`Rewrite::apply`](struct.Rewrite.html#method.apply)
    /// and returns number of new applications.
    fn apply_rewrite(
        &mut self,
        egraph: &mut EGraph<L, M>,
        rewrite: &Rewrite<L, M>,
        matches: Vec<SearchMatches>,
    ) -> usize {
        rewrite.apply(egraph, &matches).len()
    }

    /// Run the rewrites once on the egraph.
    ///
    /// It first searches all the rules using the [`search_rewrite`] wrapper.
    /// Then it applies all the rules using the [`apply_rewrite`] wrapper.
    ///
    /// ## Expectations
    ///
    /// After searching or applying a rule, this should call
    /// [`during_step`], returning immediately if it returns an error.
    /// This should _not_ call [`pre_step`] or [`post_step`], those
    /// should be called by the [`run`] method.
    ///
    /// Default implementation just calls
    /// [`Rewrite::apply`](struct.Rewrite.html#method.apply)
    /// and returns number of new applications.
    ///
    /// ## Default implementation
    ///
    /// The default implementation is probably good enough.
    /// It conforms to all the above expectations, and it performs the
    /// necessary bookkeeping to return an [`Iteration`].
    /// It additionally performs useful logging at the debug and info
    /// levels.
    /// If you're using [`env_logger`](https://docs.rs/env_logger/)
    /// (which the tests of `egg` do),
    /// see its documentation on how to see the logs.
    ///
    /// [`search_rewrite`]: trait.Runner.html#method.search_rewrite
    /// [`apply_rewrite`]: trait.Runner.html#method.apply_rewrite
    /// [`pre_step`]: trait.Runner.html#method.pre_step
    /// [`during_step`]: trait.Runner.html#method.during_step
    /// [`post_step`]: trait.Runner.html#method.post_step
    /// [`run`]: trait.Runner.html#method.run
    /// [`Iteration`]: struct.Iteration.html
    fn step(
        &mut self,
        egraph: &mut EGraph<L, M>,
        rules: &[Rewrite<L, M>],
    ) -> Result<Iteration, Self::Error> {
        let egraph_nodes = egraph.total_size();
        let egraph_classes = egraph.number_of_classes();
        trace!("EGraph {:?}", egraph.dump());

        let search_time = Instant::now();

        let mut matches = Vec::new();
        for rule in rules.iter() {
            let ms = self.search_rewrite(egraph, rule);
            matches.push(ms);
            self.during_step(egraph)?
        }

        let search_time = search_time.elapsed().as_secs_f64();
        info!("Search time: {}", search_time);

        let apply_time = Instant::now();

        let mut applied = IndexMap::new();
        for (rw, ms) in rules.iter().zip(matches) {
            let total_matches: usize = ms.iter().map(|m| m.substs.len()).sum();
            if total_matches == 0 {
                continue;
            }

            debug!("Applying {} {} times", rw.name(), total_matches);

            let actually_matched = self.apply_rewrite(egraph, rw, ms);
            if actually_matched > 0 {
                if let Some(count) = applied.get_mut(rw.name()) {
                    *count += 1;
                } else {
                    applied.insert(rw.name().to_owned(), 1);
                }
                debug!("Applied {} {} times", rw.name(), actually_matched);
            }

            self.during_step(egraph)?
        }

        let apply_time = apply_time.elapsed().as_secs_f64();
        info!("Apply time: {}", apply_time);

        let rebuild_time = Instant::now();
        egraph.rebuild();

        let rebuild_time = rebuild_time.elapsed().as_secs_f64();
        info!("Rebuild time: {}", rebuild_time);
        info!(
            "Size: n={}, e={}",
            egraph.total_size(),
            egraph.number_of_classes()
        );

        trace!("Running post_step...");
        Ok(Iteration {
            applied,
            egraph_nodes,
            egraph_classes,
            search_time,
            apply_time,
            rebuild_time,
            // best_cost,
        })
    }

    /// Run the rewrites on the egraph until an error.
    ///
    /// This should call [`pre_step`], [`step`], and [`post_step`] in
    /// a loop, in that order, until one of them returns an error.
    /// It returns the completed [`Iteration`]s and the error that
    /// caused it to stop.
    ///
    /// The default implementation does these things.
    ///
    /// [`pre_step`]: trait.Runner.html#method.pre_step
    /// [`step`]: trait.Runner.html#method.step
    /// [`post_step`]: trait.Runner.html#method.post_step
    /// [`Iteration`]: struct.Iteration.html
    fn run(
        &mut self,
        egraph: &mut EGraph<L, M>,
        rules: &[Rewrite<L, M>],
    ) -> (Vec<Iteration>, Self::Error) {
        let mut iterations = vec![];
        let mut fn_loop = || -> Result<(), Self::Error> {
            loop {
                trace!("Running pre_step...");
                self.pre_step(egraph)?;
                trace!("Running step...");
                iterations.push(self.step(egraph, rules)?);
                trace!("Running post_step...");
                self.post_step(iterations.last().unwrap(), egraph)?;
            }
        };
        let stop_reason = fn_loop().unwrap_err();
        info!("Stopping {:?}", stop_reason);
        (iterations, stop_reason)
    }

    /// Given an initial expression, make and egraph and [`run`] the
    /// rules on it.
    ///
    /// The default implementation does exactly this, also performing
    /// the bookkeeping needed to return a [`RunReport`].
    ///
    /// [`run`]: trait.Runner.html#method.run
    /// [`RunReport`]: struct.RunReport.html
    fn run_expr(
        &mut self,
        initial_expr: RecExpr<L>,
        rules: &[Rewrite<L, M>],
    ) -> (EGraph<L, M>, RunReport<L, Self::Error>) {
        // let initial_cost = calculate_cost(&initial_expr);
        // info!("Without empty: {}", initial_expr.pretty(80));

        let (mut egraph, initial_expr_eclass) = EGraph::from_expr(&initial_expr);

        let rules_time = Instant::now();
        let (iterations, stop_reason) = self.run(&mut egraph, rules);
        let rules_time = rules_time.elapsed().as_secs_f64();

        // let extract_time = Instant::now();
        // let best = Extractor::new(&egraph).find_best(root);
        // let extract_time = extract_time.elapsed().as_secs_f64();

        // info!("Extract time: {}", extract_time);
        // info!("Initial cost: {}", initial_cost);
        // info!("Final cost: {}", best.cost);
        // info!("Final: {}", best.expr.pretty(80));

        let report = RunReport {
            iterations,
            rules_time,
            // extract_time,
            stop_reason,
            // ast_size: best.expr.ast_size(),
            // ast_depth: best.expr.ast_depth(),
            initial_expr,
            initial_expr_eclass: egraph.find(initial_expr_eclass),
            // initial_cost,
            // final_cost: best.cost,
            // final_expr: best.expr,
        };
        (egraph, report)
    }
}

/** A reasonable default [`Runner`].

[`SimpleRunner`] is a [`Runner`], so it runs rewrites over an [`EGraph`].
This implementation offers several conveniences to prevent rewriting
from behaving badly and eating your computer:

- Saturation checking

  [`SimpleRunner`] checks to see if any of the rules added anything
  new to the [`EGraph`]. If none did, then it stops, returning
  [`SimpleRunnerError::Saturated`](enum.SimpleRunnerError.html#variant.Saturated).

- Iteration limits

  You can set a upper limit of iterations to do in case the search
  doesn't stop for some other reason. If this limit is hit, it stops with
  [`SimpleRunnerError::IterationLimit`](enum.SimpleRunnerError.html#variant.IterationLimit).

- [`EGraph`] size limit

  You can set a upper limit on the number of enodes in the egraph.
  If this limit is hit, it stops with
  [`SimpleRunnerError::NodeLimit`](enum.SimpleRunnerError.html#variant.NodeLimit).

- Rule backoff

  Some rules enable themselves, blowing up the [`EGraph`] and
  preventing other rewrites from running as many times.
  To prevent this, [`SimpleRunner`] implements exponentional rule backoff.

  For each rewrite, there exists a configurable initial match limit.
  If a rewrite search yield more than this limit, then we ban this
  rule for number of iterations, double its limit, and double the time
  it will be banned next time.

  This seems effective at preventing explosive rules like
  associativity from taking an unfair amount of resources.


[`SimpleRunner`]: struct.SimpleRunner.html
[`Runner`]: trait.Runner.html
[`EGraph`]: struct.EGraph.html

# Example

```
use egg::{*, rewrite as rw};

define_language! {
    enum SimpleLanguage {
        Num(i32),
        Add = "+",
        Mul = "*",
        Symbol(String),
    }
}

let rules: &[Rewrite<SimpleLanguage, ()>] = &[
    rw!("commute-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
    rw!("commute-mul"; "(* ?a ?b)" => "(* ?b ?a)"),

    rw!("add-0"; "(+ ?a 0)" => "?a"),
    rw!("mul-0"; "(* ?a 0)" => "0"),
    rw!("mul-1"; "(* ?a 1)" => "?a"),
];

let start = "(+ 0 (* 1 foo))".parse().unwrap();
// SimpleRunner is customizable in the builder pattern style.
let (egraph, report) = SimpleRunner::default()
    .with_iter_limit(10)
    .with_node_limit(10_000)
    .run_expr(start, &rules);
println!(
    "Stopped after {} iterations, reason: {:?}",
    report.iterations.len(),
    report.stop_reason
);
```
*/
pub struct SimpleRunner {
    iter_limit: usize,
    node_limit: usize,
    i: usize,
    stats: IndexMap<String, RuleStats>,
    initial_match_limit: usize,
    ban_length: usize,
}

struct RuleStats {
    times_applied: usize,
    banned_until: usize,
    times_banned: usize,
}

impl Default for SimpleRunner {
    fn default() -> Self {
        Self {
            iter_limit: 30,
            node_limit: 10_000,
            i: 0,
            stats: Default::default(),
            initial_match_limit: 1_000,
            ban_length: 5,
        }
    }
}

impl SimpleRunner {
    /// Sets the iteration limit. Default: 30
    pub fn with_iter_limit(self, iter_limit: usize) -> Self {
        Self { iter_limit, ..self }
    }

    /// Sets the egraph size limit (in enodes). Default: 10,000
    pub fn with_node_limit(self, node_limit: usize) -> Self {
        Self { node_limit, ..self }
    }

    /// Sets the initial match limit before a rule is banned. Default: 1,000
    ///
    /// Setting this to a really big number will effectively disable
    /// rule backoff.
    pub fn with_initial_match_limit(self, initial_match_limit: usize) -> Self {
        Self {
            initial_match_limit,
            ..self
        }
    }
}

/// Error returned by [`SimpleRunner`] when it stops.
///
/// [`SimpleRunner`]: struct.SimpleRunner.html
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-1", derive(serde::Serialize))]
pub enum SimpleRunnerError {
    /// The egraph saturated, i.e., there was an iteration where we
    /// didn't learn anything new from applying the rules.
    Saturated,
    /// The iteration limit was hit. The data is the iteration limit.
    IterationLimit(usize),
    /// The enode limit was hit. The data is the enode limit.
    NodeLimit(usize),
}

impl<L, M> Runner<L, M> for SimpleRunner
where
    L: Language,
    M: Metadata<L>,
{
    type Error = SimpleRunnerError;

    fn pre_step(&mut self, egraph: &mut EGraph<L, M>) -> Result<(), Self::Error> {
        info!(
            "\n\nIteration {}, n={}, e={}",
            self.i,
            egraph.total_size(),
            egraph.number_of_classes()
        );
        if self.i >= self.iter_limit {
            Err(SimpleRunnerError::IterationLimit(self.i))
        } else {
            Ok(())
        }
    }

    fn during_step(&mut self, egraph: &EGraph<L, M>) -> Result<(), Self::Error> {
        let size = egraph.total_size();
        if size > self.node_limit {
            Err(SimpleRunnerError::NodeLimit(size))
        } else {
            Ok(())
        }
    }

    fn post_step(
        &mut self,
        iteration: &Iteration,
        _egraph: &mut EGraph<L, M>,
    ) -> Result<(), Self::Error> {
        let is_banned = |s: &RuleStats| s.banned_until > self.i;
        let any_bans = self.stats.values().any(is_banned);

        self.i += 1;
        if !any_bans && iteration.applied.is_empty() {
            Err(SimpleRunnerError::Saturated)
        } else {
            Ok(())
        }
    }

    fn search_rewrite(
        &mut self,
        egraph: &mut EGraph<L, M>,
        rewrite: &Rewrite<L, M>,
    ) -> Vec<SearchMatches> {
        if let Some(limit) = self.stats.get_mut(rewrite.name()) {
            if self.i < limit.banned_until {
                debug!(
                    "Skipping {} ({}-{}), banned until {}...",
                    rewrite.name(),
                    limit.times_applied,
                    limit.times_banned,
                    limit.banned_until,
                );
                return vec![];
            }

            let matches = rewrite.search(egraph);
            let total_len: usize = matches.iter().map(|m| m.substs.len()).sum();
            let threshold = self.initial_match_limit << limit.times_banned;
            if total_len > threshold {
                let ban_length = self.ban_length << limit.times_banned;
                limit.times_banned += 1;
                limit.banned_until = self.i + ban_length;
                info!(
                    "Banning {} ({}-{}) for {} iters: {} < {}",
                    rewrite.name(),
                    limit.times_applied,
                    limit.times_banned,
                    ban_length,
                    threshold,
                    total_len,
                );
                vec![]
            } else {
                limit.times_applied += 1;
                matches
            }
        } else {
            self.stats.insert(
                rewrite.name().into(),
                RuleStats {
                    times_applied: 0,
                    banned_until: 0,
                    times_banned: 0,
                },
            );
            rewrite.search(egraph)
        }
    }
}
