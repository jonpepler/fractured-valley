//! Stage 5: a small village over a month. Twenty to fifty people on the shared mind, in one valley
//! with water, berry bushes, a deer, a boar and a few foxes, living their days: drinking, eating,
//! sleeping, talking (telling where water is, gossip, cries for help, the odd hunt). The earlier
//! stages' behaviour happens when its conditions arise, not by script.
//!
//! - `life`: the village layer round a person (the tier, gossip, the idea of a hunt);
//! - `fox`: a fox for the village (a forager that competes for berries);
//! - `runs`: the valley for a seed, the month, and what is measured (storage, time, places, talk).

pub mod fox;
pub mod life;
pub mod runs;
