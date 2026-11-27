//! Quick sweep of the betrayal over who leans to Dee, caution, seeds and temperament sets.
use fv_mind_lab::hunt::runs::*;
fn main() {
    for lean in LEANS { for caution in [10, 2] {
        let mut line = format!("{lean:?} caution {caution}: ");
        for seed in 1..=8 { for set in [0, 2] {
            let (o, _) = betrayal(&BCase { seed, set, lean, caution, honest_teller: false });
            let v: Vec<String> = o.verdicts.iter().map(|x| format!("{}", x.1.map(|j| ["A","B","C","D"][j]).unwrap_or("?"))).collect();
            line += &format!("[{}{}{} d{} g{} {}] ", o.teller.map(|t| ["A","B","C","D"][t]).unwrap_or("-"), if o.warned_at.is_some() { "w" } else { "" }, if o.counter_joined { "c" } else { "" }, o.dee_struck, o.group_struck, v.join(""));
        } }
        println!("{line}");
    } }
}
