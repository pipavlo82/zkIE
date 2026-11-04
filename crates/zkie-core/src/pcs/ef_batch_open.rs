//! S9: multi-point batch-opening verifier over EF challenge points.
//!
//! The committed tensors and Merkle roots stay base-field. The batch
//! `sum-of-products` arithmetic, the Fiat–Shamir γ, the evaluation point and
//! the opened evaluations live in `Ef`. The PCS boundary opens the base-field
//! MLEs at the EF point (`Whir::open_batch_multi_ef` / `verify_batch_multi_ef`).

use crate::common::ef_batch::{self, EfSumcheckProofBatch};
use crate::common::ef_outer::{self, eq_poly};
use crate::common::field::{Ef, Goldilocks, PrimeCharacteristicRing};
use crate::common::transcript::Transcript;
use crate::pcs::whir::{Commitment, OpeningProtocol, Proof, ProverData, Whir};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EfBatchOpeningProof {
    pub sumcheck: EfSumcheckProofBatch,
    pub open: Proof,
    pub evals: Vec<Ef>,
}

/// Unique tables in first-appearance order plus each claim's group index.
fn claim_groups(table_idxs: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut unique = Vec::new();
    let mut group_of = Vec::with_capacity(table_idxs.len());
    for &t in table_idxs {
        match unique.iter().position(|&u| u == t) {
            Some(i) => group_of.push(i),
            None => {
                unique.push(t);
                group_of.push(unique.len() - 1);
            }
        }
    }
    (unique, group_of)
}

/// Prove a multi-point batch opening at EF points. `values` holds exactly one
/// base-field tensor per unique table, in first-appearance order.
#[allow(clippy::too_many_arguments)]
pub fn prove_batch_open_committed_ef(
    whir: &Whir,
    commitment: &Commitment,
    protocol: &OpeningProtocol,
    num_tables: usize,
    prover_data: ProverData,
    table_idxs: &[usize],
    values: Vec<Vec<Goldilocks>>,
    points: &[Vec<Ef>],
    claimed: &[Ef],
) -> Result<EfBatchOpeningProof, String> {
    let n = points.len();
    if table_idxs.len() != n || claimed.len() != n || n == 0 {
        return Err("EF batch-open length mismatch".to_owned());
    }
    let (unique, group_of) = claim_groups(table_idxs);
    if values.len() != unique.len() {
        return Err("EF batch-open unique table mismatch".to_owned());
    }
    let d = points[0].len();
    if points.iter().any(|p| p.len() != d) {
        return Err("EF batch-open point arity mismatch".to_owned());
    }
    let table_len = 1usize
        .checked_shl(d as u32)
        .ok_or_else(|| "EF batch-open arity overflow".to_owned())?;
    if values.iter().any(|v| v.len() != table_len) {
        return Err("EF batch-open tensor/point arity mismatch".to_owned());
    }
    if table_idxs.iter().any(|&t| t >= num_tables) {
        return Err("EF batch-open table index out of range".to_owned());
    }

    let mut t = Transcript::new(b"zkie-batch-open");
    t.absorb(&bincode::serialize(commitment).map_err(|e| e.to_string())?);
    for (pt, c) in points.iter().zip(claimed.iter()) {
        t.absorb_efs(pt);
        t.absorb_ef(*c);
    }
    let gamma = t.ef_field();

    let mut weights: Vec<Vec<Ef>> =
        (0..unique.len()).map(|_| vec![Ef::ZERO; table_len]).collect();
    let mut gamma_pow = Ef::ONE;
    let mut claimed_sum = Ef::ZERO;
    for i in 0..n {
        let eq = ef_outer::eq_evals(&points[i]);
        if eq.len() != table_len {
            return Err("EF batch-open eq length mismatch".to_owned());
        }
        let w = &mut weights[group_of[i]];
        for (acc, &e) in w.iter_mut().zip(eq.iter()) {
            *acc = *acc + gamma_pow * e;
        }
        claimed_sum = claimed_sum + gamma_pow * claimed[i];
        gamma_pow = gamma_pow * gamma;
    }
    let values_ef: Vec<Vec<Ef>> = values
        .into_iter()
        .map(|v| v.into_iter().map(Ef::from).collect())
        .collect();
    let (sumcheck, challenges) =
        ef_batch::prove_sum_of_products(&weights, &values_ef, claimed_sum, &mut t);
    let (open, evals) = whir.open_batch_multi_ef(prover_data, protocol, num_tables, &challenges);
    Ok(EfBatchOpeningProof { sumcheck, open, evals })
}

/// Verify an EF multi-point batch opening. Reads no committed tensor data.
#[allow(clippy::too_many_arguments)]
pub fn verify_batch_open_committed_ef(
    whir: &Whir,
    commitment: &Commitment,
    protocol: &OpeningProtocol,
    num_tables: usize,
    table_idxs: &[usize],
    points: &[Vec<Ef>],
    claimed: &[Ef],
    proof: &EfBatchOpeningProof,
) -> bool {
    let n = table_idxs.len();
    if n != points.len() || n != claimed.len() || n == 0 {
        return false;
    }
    let d = points[0].len();
    if points.iter().any(|p| p.len() != d) {
        return false;
    }
    if table_idxs.iter().any(|&t| t >= num_tables) {
        return false;
    }
    let (unique, group_of) = claim_groups(table_idxs);

    let mut t = Transcript::new(b"zkie-batch-open");
    let Ok(commitment_bytes) = bincode::serialize(commitment) else { return false };
    t.absorb(&commitment_bytes);
    for (pt, c) in points.iter().zip(claimed.iter()) {
        t.absorb_efs(pt);
        t.absorb_ef(*c);
    }
    let gamma = t.ef_field();

    let mut claimed_sum = Ef::ZERO;
    let mut gamma_pow = Ef::ONE;
    for i in 0..n {
        claimed_sum = claimed_sum + gamma_pow * claimed[i];
        gamma_pow = gamma_pow * gamma;
    }
    let (rounds_ok, challenges) =
        ef_batch::verify_sum_of_products(&proof.sumcheck, claimed_sum, d, &mut t);
    if !rounds_ok {
        return false;
    }
    let evals = match whir.verify_batch_multi_ef(commitment, &proof.open, protocol, num_tables, &challenges) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if evals != proof.evals {
        return false;
    }
    let mut w_at_r = vec![Ef::ZERO; unique.len()];
    let mut gamma_pow = Ef::ONE;
    for i in 0..n {
        let Some(eq) = eq_poly(&points[i], &challenges) else { return false };
        w_at_r[group_of[i]] = w_at_r[group_of[i]] + gamma_pow * eq;
        gamma_pow = gamma_pow * gamma;
    }
    let mut expected = Ef::ZERO;
    for g in 0..unique.len() {
        expected = expected + w_at_r[g] * evals[unique[g]];
    }
    proof.sumcheck.b_eval == expected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::field::XorShift64;
    use crate::pcs::whir::Whir;

    fn ef_eval_at(values: &[Goldilocks], point: &[Ef]) -> Ef {
        // EF evaluation of a base-field MLE via the core helper.
        crate::common::ef_outer::mle_eval_at_ef(values, point).unwrap()
    }

    #[test]
    fn ef_batch_open_roundtrip_and_tamper() {
        let mut rng = XorShift64::new(0x9401);
        let d = 5usize;
        let n = 3usize;
        let tables: Vec<Vec<Goldilocks>> = (0..n)
            .map(|_| (0..1 << d).map(|_| rng.field()).collect())
            .collect();
        let refs: Vec<&[Goldilocks]> = tables.iter().map(Vec::as_slice).collect();
        let base = Whir::new_testing(d);
        let (commitment, prover_data, protocol, batch_whir) = base.commit_batch(&refs);
        let point: Vec<Ef> = (0..d).map(|_| Ef::new([rng.field(), rng.field()])).collect();
        let claimed: Vec<Ef> = tables.iter().map(|t| ef_eval_at(t, &point)).collect();
        let table_idxs = vec![0usize, 1, 2, 0];
        let points = vec![point.clone(), point.clone(), point.clone(), point.clone()];
        let claimed = vec![claimed[0], claimed[1], claimed[2], claimed[0]];
        let values = vec![tables[0].clone(), tables[1].clone(), tables[2].clone()];
        let proof = prove_batch_open_committed_ef(
            &batch_whir, &commitment, &protocol, n, prover_data,
            &table_idxs, values, &points, &claimed,
        ).expect("prove EF batch open");
        assert!(verify_batch_open_committed_ef(
            &batch_whir, &commitment, &protocol, n, &table_idxs, &points, &claimed, &proof));
        assert_eq!(proof.evals.len(), n);
        // The WHIR-level single-table EF open/verify roundtrip is the direct
        // EF-point opening: it must equal the EF MLE evaluation of the F table.
        let (single_commit, single_pd, single_protocol) = base.commit(&tables[0]);
        let (op, opened) = base.open_ef(single_pd, &single_protocol, &point);
        assert_eq!(opened, ef_eval_at(&tables[0], &point));
        assert_eq!(base.verify_ef(&single_commit, &op, &single_protocol, &point).unwrap(), opened);

        let mut bad = proof.clone();
        bad.sumcheck.rounds[0].c0 += Ef::ONE;
        assert!(!verify_batch_open_committed_ef(
            &batch_whir, &commitment, &protocol, n, &table_idxs, &points, &claimed, &bad));
        let mut bad2 = proof.clone();
        bad2.evals[0] += Ef::ONE;
        assert!(!verify_batch_open_committed_ef(
            &batch_whir, &commitment, &protocol, n, &table_idxs, &points, &claimed, &bad2));
    }
}
