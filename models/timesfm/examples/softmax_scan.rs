//! Scan every Softmax op in the real TimesFM graph and report the maximum
//! absolute row sum S. Used to size the committed Softmax remainder window.
//!
//! Usage: cargo run --release -p zkie-models-timesfm --example softmax_scan

use zkie_models_timesfm::build_timesfm;
use zkie_ops::compose::{evaluate_ops, Op, Store};

fn main() {
    let mut store = Store::new();
    let mut ops = Vec::new();
    let _ = build_timesfm(&mut store, &mut ops, "models/timesfm/weights", 16);
    evaluate_ops(&mut store, &ops);
    let mut max_s: i64 = 0;
    let mut max_at = (0usize, 0usize);
    let mut min_s: i64 = 0;
    let mut count = 0usize;
    for (op_index, op) in ops.iter().enumerate() {
        let Op::Softmax { e, m, n, .. } = op else { continue };
        let ev = store.get(*e);
        if ev.len() != m * n {
            eprintln!("softmax op {op_index}: e len {} != {}x{}", ev.len(), m, n);
            std::process::exit(2);
        }
        count += 1;
        for i in 0..*m {
            let s: i64 = (0..*n)
                .map(|j| zkie_core::common::fixed_point::to_i64(ev[i * n + j]))
                .sum();
            if s.abs() > max_s {
                max_s = s.abs();
                max_at = (op_index, i);
            }
            if s < min_s { min_s = s; }
        }
    }
    println!("timesfm softmax_ops={count} max_abs_S={max_s} at_op_row={max_at:?} min_S={min_s}");
    let bit = if max_s <= 1 { 0 } else {
        (usize::BITS - ((max_s - 1) as u64).leading_zeros()) as usize
    };
    println!("timesfm ceil_log2(max_abs_S)={bit} next_pow2={}", 1u64 << bit);
}
