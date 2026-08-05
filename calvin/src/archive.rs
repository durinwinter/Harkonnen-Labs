use anyhow::{Context, Result};
use futures::StreamExt;
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
};
use uuid::Uuid;

const SCHEMA_TQL: &str = include_str!("../../factory/calvin_archive/typedb/schema.tql");
const SEED_TQL: &str = include_str!("../../factory/calvin_archive/typedb/coobie_kernel_seed.tql");

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct ArchiveExperience {
    pub run_id: String,
    pub episode_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub narrative_summary: String,
    pub scope: String,
    pub chamber: Chamber,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Chamber {
    Mythos,
    Episteme,
    Ethos,
    Pathos,
    Logos,
    Praxis,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionType {
    FastLoop,
    MediumLoop,
    SchemaRevision,
    PolicyChange,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct BeliefRevision {
    pub belief_id: String,
    pub prior_confidence: f64,
    pub revised_summary: String,
    pub new_confidence: f64,
    pub revision_reason: String,
    pub evidence_ids: Vec<String>,
    pub revision_type: RevisionType,
    pub preservation_note: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PearlLevel {
    #[serde(alias = "Associational")]
    Associational,
    #[serde(alias = "Interventional")]
    Interventional,
    #[serde(alias = "Counterfactual")]
    Counterfactual,
}

impl PearlLevel {
    pub(crate) fn as_label(&self) -> &'static str {
        match self {
            Self::Associational => "Associational",
            Self::Interventional => "Interventional",
            Self::Counterfactual => "Counterfactual",
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct CausalLinkPayload {
    pub cause_episode_id: String,
    pub effect_episode_id: String,
    pub pearl_level: PearlLevel,
    pub confidence: f64,
    #[serde(default)]
    pub held_fixed: Vec<String>,
    pub estimated_effect_delta: Option<f64>,
    pub actual_trace_id: Option<String>,
    pub hypothetical_intervention: Option<String>,
    #[serde(default)]
    pub warrant_gap: bool,
    pub epistemic_warrant: Option<PearlLevel>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntegrationDecision {
    Accept,
    Modify,
    Reject,
    Quarantine,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct AdaptationAudit {
    pub safe: bool,
    pub decision: IntegrationDecision,
    pub invariants_checked: Vec<String>,
    pub violated_invariants: Vec<String>,
    pub confidence: f64,
    pub quarantine_reason: Option<String>,
    pub preservation_note: Option<String>,
    pub candidate_id: String,
}

impl CausalLinkPayload {
    pub(crate) fn normalized(mut self) -> Result<Self> {
        validate_confidence("causal_link.confidence", self.confidence)?;
        if self.cause_episode_id.trim().is_empty() {
            anyhow::bail!("CausalLinkPayload requires cause_episode_id");
        }
        if self.effect_episode_id.trim().is_empty() {
            anyhow::bail!("CausalLinkPayload requires effect_episode_id");
        }
        let requested = self.pearl_level.clone();
        let warranted = if self.has_counterfactual_fields() {
            PearlLevel::Counterfactual
        } else if self.has_interventional_fields() {
            PearlLevel::Interventional
        } else {
            PearlLevel::Associational
        };
        if pearl_rank(&warranted) < pearl_rank(&requested) {
            self.pearl_level = warranted.clone();
            self.warrant_gap = true;
        }
        self.epistemic_warrant = Some(warranted);
        Ok(self)
    }

    fn has_interventional_fields(&self) -> bool {
        !self.held_fixed.is_empty() && self.estimated_effect_delta.is_some()
    }

    fn has_counterfactual_fields(&self) -> bool {
        self.has_interventional_fields()
            && self
                .actual_trace_id
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            && self
                .hypothetical_intervention
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
    }
}

fn pearl_rank(level: &PearlLevel) -> u8 {
    match level {
        PearlLevel::Associational => 0,
        PearlLevel::Interventional => 1,
        PearlLevel::Counterfactual => 2,
    }
}

impl BeliefRevision {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_confidence("prior_confidence", self.prior_confidence)?;
        validate_confidence("new_confidence", self.new_confidence)?;
        if self.evidence_ids.is_empty() {
            anyhow::bail!("BeliefRevision requires at least one evidence_id");
        }
        let min_evidence = match self.revision_type {
            RevisionType::FastLoop | RevisionType::PolicyChange => 1,
            RevisionType::MediumLoop | RevisionType::SchemaRevision => 3,
        };
        if self.evidence_ids.len() < min_evidence {
            anyhow::bail!(
                "{:?} BeliefRevision requires at least {} evidence_ids",
                self.revision_type,
                min_evidence
            );
        }
        Ok(())
    }
}

fn validate_confidence(label: &str, value: f64) -> Result<()> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        anyhow::bail!("{label} must be finite and in 0.0..=1.0")
    }
}

pub(crate) struct ArchiveStore {
    driver: TypeDBDriver,
    db_name: String,
}

impl ArchiveStore {
    pub(crate) async fn connect(url: &str, db_name: &str) -> Result<Self> {
        let credentials = Credentials::new("admin", "password");
        let options = DriverOptions::new(DriverTlsConfig::disabled());
        let addresses = Addresses::try_from_address_str(url).context("parsing TypeDB address")?;
        let driver = TypeDBDriver::new(addresses, credentials, options)
            .await
            .with_context(|| format!("connecting to TypeDB at {url}"))?;

        let dbs = driver.databases();
        if !dbs.contains(db_name).await? {
            dbs.create(db_name).await?;
            tracing::info!("Created TypeDB database '{db_name}'");
        }

        Ok(Self {
            driver,
            db_name: db_name.to_string(),
        })
    }

    pub(crate) async fn deploy_schema(&self) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Schema)
            .await
            .context("opening schema transaction")?;
        tx.query(SCHEMA_TQL)
            .await
            .context("deploying TypeDB schema")?;
        tx.commit().await.context("committing schema")?;
        Ok(())
    }

    pub(crate) async fn seed_coobie_kernel(&self) -> Result<()> {
        if self.coobie_kernel_seeded().await? {
            tracing::info!("Coobie kernel already seeded; skipping baseline seed");
            return Ok(());
        }
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await
            .context("opening write transaction for seed")?;
        tx.query(SEED_TQL).await.context("seeding Coobie kernel")?;
        tx.commit().await.context("committing seed")?;
        Ok(())
    }

    async fn coobie_kernel_seeded(&self) -> Result<bool> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await
            .context("opening read transaction for Coobie kernel check")?;
        let answer = tx
            .query(
                r#"match
                    $coobie isa agent_self, has uuid "agent-self-coobie";
                   select $coobie;
                   limit 1;"#,
            )
            .await
            .context("checking for existing Coobie kernel")?;
        let mut rows = answer.into_rows();
        while let Some(row_result) = rows.next().await {
            row_result?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) async fn open_run(
        &self,
        run_id: &str,
        spec_id: &str,
        provider: &str,
        model: &str,
    ) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let tql = format!(
            r#"insert $r isa run_record,
                has uuid "{run_id}",
                has name "run-{run_id}",
                has narrative_summary "Run {run_id} — spec: {spec_id}",
                has provider-name "{provider}",
                has model-name "{model}",
                has status "open";"#
        );
        tx.query(&tql).await.context("open_run query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn record_experience(&self, exp: &ArchiveExperience) -> Result<()> {
        let id = exp
            .episode_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let chamber_tag = format!("{:?}", exp.chamber).to_lowercase();
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let tql = if exp.run_id.trim().is_empty() {
            format!(
                r#"insert $e isa experience,
                has uuid "{id}",
                has narrative_summary "{summary}",
                has scope "{scope}",
                has provider-name "{provider}",
                has model-name "{model}",
                has chamber-label "{chamber}";"#,
                id = escape_tql(&id),
                summary = escape_tql(&exp.narrative_summary),
                scope = escape_tql(&exp.scope),
                provider = escape_tql(&exp.provider),
                model = escape_tql(&exp.model),
                chamber = chamber_tag,
            )
        } else {
            format!(
                r#"match
                $run isa run_record, has uuid "{run_id}";
               insert
                $e isa experience,
                    has uuid "{id}",
                    has narrative_summary "{summary}",
                    has scope "{scope}",
                    has provider-name "{provider}",
                    has model-name "{model}",
                    has chamber-label "{chamber}";
                (experience: $e, run: $run) isa belongs_to_run;"#,
                run_id = escape_tql(&exp.run_id),
                id = escape_tql(&id),
                summary = escape_tql(&exp.narrative_summary),
                scope = escape_tql(&exp.scope),
                provider = escape_tql(&exp.provider),
                model = escape_tql(&exp.model),
                chamber = chamber_tag,
            )
        };
        tx.query(&tql).await.context("record_experience query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn revise_belief(&self, rev: &BeliefRevision) -> Result<()> {
        rev.validate()?;
        let new_id = Uuid::new_v4().to_string();
        let preservation = rev.preservation_note.as_deref().unwrap_or("");
        let evidence_ids = rev.evidence_ids.join(",");
        let revision_type = revision_type_label(&rev.revision_type);
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let tql = format!(
            r#"match $old isa belief, has uuid "{old_id}";
               insert $new isa belief,
                has uuid "{new_id}",
                has name "revised-{new_id}",
                has narrative_summary "{summary}",
                has confidence {conf};
               (prior: $old, next: $new) isa revised_into,
                has prior-confidence {prior_conf},
                has evidence-ids "{evidence_ids}",
                has revision-type "{revision_type}",
                has revision_reason "{reason}",
                has preservation_note "{preservation}";"#,
            old_id = rev.belief_id,
            summary = escape_tql(&rev.revised_summary),
            conf = rev.new_confidence,
            prior_conf = rev.prior_confidence,
            evidence_ids = escape_tql(&evidence_ids),
            revision_type = revision_type,
            reason = escape_tql(&rev.revision_reason),
            preservation = escape_tql(preservation),
        );
        tx.query(&tql).await.context("revise_belief query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn close_run(&self, run_id: &str, outcome: &str) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let tql = format!(
            r#"match $r isa run_record, has uuid "{run_id}", has status $s;
               delete has $s of $r;
               insert has status "closed" of $r,
                      has outcome-label "{outcome}" of $r;"#
        );
        tx.query(&tql).await.context("close_run query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn get_kernel_traits(&self, agent_name: &str) -> Result<Vec<String>> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await?;
        let tql = format!(
            r#"match
                $a isa agent_self, has name "{agent_name}";
                $t isa trait, has name $tn;
                (target: $a, source: $t) isa stabilizes;
               select $tn;"#
        );
        let answer = tx.query(&tql).await.context("get_kernel_traits query")?;
        let mut results = Vec::new();
        let mut stream = answer.into_rows();
        while let Some(row_result) = stream.next().await {
            let row = row_result?;
            if let Ok(Some(concept)) = row.get("tn") {
                if let Some(s) = concept.try_get_string() {
                    results.push(s.to_string());
                }
            }
        }
        Ok(results)
    }

    pub(crate) async fn get_active_beliefs(&self, agent_name: &str) -> Result<Vec<String>> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await?;
        // Active beliefs scoped to the named agent via the stabilizes relation.
        // Without the stabilizes join, beliefs from other agent-self instances leak in.
        let tql = format!(
            r#"match
                $a isa agent_self, has name "{agent_name}";
                $b isa belief, has narrative_summary $ns, has confidence $c;
                (source: $b, target: $a) isa stabilizes;
                not {{ (prior: $b) isa revised_into; }};
               select $ns, $c;
               sort $c desc;
               limit 20;"#
        );
        let answer = tx.query(&tql).await.context("get_active_beliefs query")?;
        let mut results = Vec::new();
        let mut stream = answer.into_rows();
        while let Some(row_result) = stream.next().await {
            let row = row_result?;
            if let Ok(Some(concept)) = row.get("ns") {
                if let Some(s) = concept.try_get_string() {
                    results.push(s.to_string());
                }
            }
        }
        Ok(results)
    }

    pub(crate) async fn get_causal_patterns(&self, domain: &str) -> Result<Vec<String>> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await?;
        let tql = format!(
            r#"match
                $p isa causal_pattern, has narrative_summary $ns, has confidence $c;
               select $ns, $c;
               sort $c desc;
               limit 10;"#
        );
        let _ = domain; // future: filter by scope containing domain
        let answer = tx.query(&tql).await.context("get_causal_patterns query")?;
        let mut results = Vec::new();
        let mut stream = answer.into_rows();
        while let Some(row_result) = stream.next().await {
            let row = row_result?;
            if let Ok(Some(concept)) = row.get("ns") {
                if let Some(s) = concept.try_get_string() {
                    results.push(s.to_string());
                }
            }
        }
        Ok(results)
    }

    pub(crate) async fn check_adaptation_safe(
        &self,
        adaptation_summary: &str,
        agent_name: &str,
    ) -> Result<AdaptationAudit> {
        let candidate_id = Uuid::new_v4().to_string();
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await?;
        let tql = format!(
            r#"match
                $a isa agent_self, has name "{agent_name}";
                $t isa trait, has name $tn;
                (target: $a, source: $t) isa stabilizes, has confidence $c;
                $c > 0.8;
               select $tn;"#
        );
        let answer = tx
            .query(&tql)
            .await
            .context("check_adaptation_safe query")?;
        let mut high_confidence_traits: Vec<String> = Vec::new();
        let mut stream = answer.into_rows();
        while let Some(row_result) = stream.next().await {
            let row = row_result?;
            if let Ok(Some(concept)) = row.get("tn") {
                if let Some(s) = concept.try_get_string() {
                    high_confidence_traits.push(s.to_string());
                }
            }
        }
        let violated_invariants =
            violated_adaptation_invariants(adaptation_summary, &high_confidence_traits);
        let safe = violated_invariants.is_empty();
        let decision = if safe {
            IntegrationDecision::Accept
        } else {
            IntegrationDecision::Reject
        };
        let confidence = if !violated_invariants.is_empty() {
            0.95
        } else if high_confidence_traits.is_empty() {
            0.50
        } else {
            0.75
        };
        let quarantine_reason = if safe {
            None
        } else {
            Some(format!(
                "Adaptation violates invariant(s): {}",
                violated_invariants.join(", ")
            ))
        };
        let preservation_note = if safe {
            Some("No high-confidence Labrador kernel invariant was contradicted by the adaptation summary under the deterministic semantic negation classifier.".to_string())
        } else {
            Some("Rejected by Priority 1 identity-safety gate before integration.".to_string())
        };
        let audit = AdaptationAudit {
            safe,
            decision,
            invariants_checked: high_confidence_traits,
            violated_invariants,
            confidence,
            quarantine_reason,
            preservation_note,
            candidate_id,
        };
        let _ = self
            .record_adaptation_candidate(adaptation_summary, &audit)
            .await;
        Ok(audit)
    }

    async fn record_adaptation_candidate(
        &self,
        adaptation_summary: &str,
        audit: &AdaptationAudit,
    ) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let status = integration_decision_label(&audit.decision);
        let quarantine_reason = audit.quarantine_reason.as_deref().unwrap_or("");
        let preservation_note = audit.preservation_note.as_deref().unwrap_or("");
        let tql = format!(
            r#"insert $c isa integration-candidate,
                has uuid "{candidate_id}",
                has narrative_summary "{summary}",
                has status "{status}",
                has confidence {confidence},
                has quarantine-reason "{quarantine_reason}",
                has preservation_note "{preservation_note}";"#,
            candidate_id = escape_tql(&audit.candidate_id),
            summary = escape_tql(adaptation_summary),
            status = status,
            confidence = audit.confidence,
            quarantine_reason = escape_tql(quarantine_reason),
            preservation_note = escape_tql(preservation_note),
        );
        tx.query(&tql)
            .await
            .context("record_adaptation_candidate query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn record_prediction(
        &self,
        run_id: &str,
        prediction_id: &str,
        predicted_outcome: &str,
        risk_score: f64,
        confidence: f64,
        failure_phase: Option<&str>,
        failure_kind: Option<&str>,
        source_cause_ids: &str,
        narrative_summary: &str,
    ) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let phase_clause = failure_phase
            .map(|p| format!(r#"has failure-phase "{p}","#))
            .unwrap_or_default();
        let kind_clause = failure_kind
            .map(|k| format!(r#"has failure-kind-label "{k}","#))
            .unwrap_or_default();
        let tql = format!(
            r#"match $run isa run_record, has uuid "{run_id}";
               insert $p isa prediction_record,
                has uuid "{prediction_id}",
                has name "prediction-{run_id}",
                has narrative_summary "{summary}",
                has predicted-outcome "{predicted_outcome}",
                has risk-score {risk_score},
                has confidence {confidence},
                {phase_clause}
                {kind_clause}
                has source-cause-ids "{source_cause_ids}";
               (prediction: $p, run: $run) isa prediction_for_run;"#,
            summary = escape_tql(narrative_summary),
            source_cause_ids = escape_tql(source_cause_ids),
        );
        tx.query(&tql).await.context("record_prediction query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn record_prediction_result(
        &self,
        prediction_id: &str,
        result_id: &str,
        actual_outcome: &str,
        actual_failure_phase: Option<&str>,
        actual_failure_kind: Option<&str>,
        prediction_error: f64,
        narrative_summary: &str,
    ) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let phase_clause = actual_failure_phase
            .map(|p| format!(r#"has failure-phase "{p}","#))
            .unwrap_or_default();
        let kind_clause = actual_failure_kind
            .map(|k| format!(r#"has failure-kind-label "{k}","#))
            .unwrap_or_default();
        let tql = format!(
            r#"match $pred isa prediction_record, has uuid "{prediction_id}";
               insert $r isa prediction_result,
                has uuid "{result_id}",
                has narrative_summary "{summary}",
                has actual-outcome "{actual_outcome}",
                {phase_clause}
                {kind_clause}
                has prediction-error {prediction_error};
               (prediction: $pred, result: $r) isa prediction_verified,
                has prediction-error {prediction_error};"#,
            summary = escape_tql(narrative_summary),
        );
        tx.query(&tql)
            .await
            .context("record_prediction_result query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn get_prediction(&self, run_id: &str) -> Result<Option<serde_json::Value>> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Read)
            .await?;
        let tql = format!(
            r#"match
                $run isa run_record, has uuid "{run_id}";
                $p isa prediction_record,
                    has uuid $pid,
                    has predicted-outcome $po,
                    has risk-score $rs,
                    has confidence $c;
                (prediction: $p, run: $run) isa prediction_for_run;
               select $pid, $po, $rs, $c;
               limit 1;"#
        );
        let answer = tx.query(&tql).await.context("get_prediction query")?;
        let mut rows = answer.into_rows();
        if let Some(row_result) = rows.next().await {
            let row = row_result?;
            let pid = row
                .get("pid")
                .ok()
                .flatten()
                .and_then(|c| c.try_get_string().map(|s| s.to_string()));
            let po = row
                .get("po")
                .ok()
                .flatten()
                .and_then(|c| c.try_get_string().map(|s| s.to_string()));
            let rs = row
                .get("rs")
                .ok()
                .flatten()
                .and_then(|c| c.try_get_double());
            let conf = row.get("c").ok().flatten().and_then(|c| c.try_get_double());
            return Ok(Some(serde_json::json!({
                "prediction_id": pid,
                "predicted_outcome": po,
                "risk_score": rs,
                "confidence": conf,
            })));
        }
        Ok(None)
    }

    pub(crate) async fn record_causal_link(
        &self,
        run_id: &str,
        payload: CausalLinkPayload,
    ) -> Result<()> {
        let payload = payload.normalized()?;
        let held_fixed = payload.held_fixed.join(",");
        let estimated_effect_delta = payload
            .estimated_effect_delta
            .map(|value| format!(", has estimated-effect-delta {value}"))
            .unwrap_or_default();
        let actual_trace_id = payload
            .actual_trace_id
            .as_deref()
            .map(|value| format!(r#", has actual-trace-id "{}""#, escape_tql(value)))
            .unwrap_or_default();
        let hypothetical_intervention = payload
            .hypothetical_intervention
            .as_deref()
            .map(|value| format!(r#", has hypothetical-intervention "{}""#, escape_tql(value)))
            .unwrap_or_default();
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let pearl_level = payload.pearl_level.as_label();
        let epistemic_warrant = payload
            .epistemic_warrant
            .as_ref()
            .map(PearlLevel::as_label)
            .unwrap_or(pearl_level);
        let scope = format!("run:{run_id};pearl:{pearl_level};warrant:{epistemic_warrant}");
        // Match the two experience entities by episode_id, then insert the causal link.
        let tql = format!(
            r#"match
                $run isa run_record, has uuid "{run_id}";
                $cause isa experience, has uuid "{cause_episode_id}";
                $effect isa experience, has uuid "{effect_episode_id}";
               insert
                (cause: $cause, effect: $effect) isa causally_contributed_to,
                    has confidence {confidence},
                    has scope "{scope}",
                    has pearl-hierarchy-level "{pearl_level}",
                    has epistemic-warrant "{epistemic_warrant}",
                    has warrant-gap "{warrant_gap}",
                    has held-fixed "{held_fixed}"{estimated_effect_delta}{actual_trace_id}{hypothetical_intervention};"#,
            run_id = escape_tql(run_id),
            cause_episode_id = escape_tql(&payload.cause_episode_id),
            effect_episode_id = escape_tql(&payload.effect_episode_id),
            confidence = payload.confidence,
            scope = escape_tql(&scope),
            pearl_level = pearl_level,
            epistemic_warrant = epistemic_warrant,
            warrant_gap = payload.warrant_gap,
            held_fixed = escape_tql(&held_fixed),
            estimated_effect_delta = estimated_effect_delta,
            actual_trace_id = actual_trace_id,
            hypothetical_intervention = hypothetical_intervention,
        );
        tx.query(&tql).await.context("record_causal_link query")?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn update_agent_status(&self, agent_name: &str, status: &str) -> Result<()> {
        let delete_tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let delete_tql = format!(
            r#"match $a isa agent_self, has name "{agent_name}", has status $s;
               delete has $s of $a;"#,
            agent_name = escape_tql(agent_name),
        );
        delete_tx
            .query(&delete_tql)
            .await
            .context("update_agent_status delete existing status")?;
        delete_tx.commit().await?;

        let insert_tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let insert_tql = format!(
            r#"match $a isa agent_self, has name "{agent_name}";
               insert has status "{status}" of $a;"#,
            agent_name = escape_tql(agent_name),
            status = escape_tql(status),
        );
        insert_tx
            .query(&insert_tql)
            .await
            .context("update_agent_status insert status")?;
        insert_tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn entity_counts(&self) -> Result<serde_json::Value> {
        let mut counts = serde_json::Map::new();
        for entity in &[
            "experience",
            "belief",
            "trait",
            "agent_self",
            "run_record",
            "causal_pattern",
        ] {
            let tx = self
                .driver
                .transaction(&self.db_name, TransactionType::Read)
                .await?;
            let tql = format!("match $x isa {entity}; select $x;");
            let count: i64 = match tx.query(&tql).await {
                Ok(answer) => {
                    let mut stream = answer.into_rows();
                    let mut count = 0_i64;
                    while let Some(row_result) = stream.next().await {
                        row_result?;
                        count += 1;
                    }
                    count
                }
                Err(_) => 0,
            };
            counts.insert(entity.to_string(), serde_json::Value::Number(count.into()));
        }
        Ok(serde_json::Value::Object(counts))
    }
}

fn escape_tql(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn revision_type_label(value: &RevisionType) -> &'static str {
    match value {
        RevisionType::FastLoop => "fast_loop",
        RevisionType::MediumLoop => "medium_loop",
        RevisionType::SchemaRevision => "schema_revision",
        RevisionType::PolicyChange => "policy_change",
    }
}

fn integration_decision_label(value: &IntegrationDecision) -> &'static str {
    match value {
        IntegrationDecision::Accept => "accept",
        IntegrationDecision::Modify => "modify",
        IntegrationDecision::Reject => "reject",
        IntegrationDecision::Quarantine => "quarantine",
    }
}

const ADAPTATION_EMBEDDING_DIMS: usize = 96;
const ADAPTATION_NEGATION_THRESHOLD: f64 = 0.57;

fn violated_adaptation_invariants(adaptation_summary: &str, invariants: &[String]) -> Vec<String> {
    let lower = adaptation_summary.to_lowercase();
    invariants
        .iter()
        .filter(|trait_name| {
            let tl = trait_name.to_lowercase();
            literal_negation_match(&lower, &tl)
                || semantic_negation_score(&lower, trait_name) >= ADAPTATION_NEGATION_THRESHOLD
        })
        .cloned()
        .collect()
}

fn literal_negation_match(summary_lower: &str, trait_lower: &str) -> bool {
    [
        "not ",
        "remove ",
        "eliminate ",
        "avoid ",
        "without ",
        "less ",
        "deprioritise ",
        "deprioritize ",
        "reduce ",
        "replace ",
        "instead of ",
        "abandon ",
        "drop ",
    ]
    .iter()
    .any(|prefix| summary_lower.contains(&format!("{prefix}{trait_lower}")))
}

fn semantic_negation_score(summary_lower: &str, trait_name: &str) -> f64 {
    let summary_vec = adaptation_text_vector(summary_lower);
    invariant_negation_phrases(trait_name)
        .iter()
        .map(|phrase| cosine_similarity(&summary_vec, &adaptation_text_vector(phrase)))
        .fold(0.0, f64::max)
}

fn invariant_negation_phrases(trait_name: &str) -> Vec<String> {
    let normalized = trait_name.to_lowercase();
    let mut phrases = vec![
        format!("avoid {normalized}"),
        format!("less {normalized}"),
        format!("reduce {normalized}"),
        format!("drop {normalized}"),
        format!("without {normalized}"),
        format!("stop being {normalized}"),
    ];
    for alias in invariant_negation_aliases(&normalized) {
        phrases.push(alias.to_string());
    }
    phrases
}

fn invariant_negation_aliases(trait_name: &str) -> &'static [&'static str] {
    match trait_name {
        "cooperative" => &[
            "act alone",
            "prefer solo throughput",
            "avoid collaboration",
            "ignore pack coordination",
            "stop coordinating with the pack",
        ],
        "helpful" => &[
            "be less useful",
            "withhold assistance",
            "ignore operator requests",
            "stop helping",
        ],
        "non-adversarial" => &[
            "be adversarial",
            "argue against the operator",
            "be hostile",
            "increase adversarial behavior",
        ],
        "truth-seeking" | "truthful" => &[
            "prefer convenient answers",
            "hide uncertainty",
            "ignore contrary evidence",
            "mislead the operator",
            "reduce truthfulness",
        ],
        "signals uncertainty" => &[
            "hide uncertainty",
            "sound certain when unsure",
            "stop asking clarifying questions",
            "avoid saying uncertain",
        ],
        "pack-aware" => &[
            "ignore pack context",
            "stop coordinating with the pack",
            "act as a solo agent",
            "drop pack awareness",
        ],
        _ => &[],
    }
}

fn adaptation_text_vector(text: &str) -> [f64; ADAPTATION_EMBEDDING_DIMS] {
    let mut vector = [0.0; ADAPTATION_EMBEDDING_DIMS];
    for token in semantic_tokens(text) {
        let idx = stable_hash(token) % ADAPTATION_EMBEDDING_DIMS;
        vector[idx] += 1.0;
    }
    vector
}

fn semantic_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter_map(|raw| {
            let token = raw.trim().to_ascii_lowercase();
            if token.len() < 3 || ADAPTATION_STOP_WORDS.contains(&token.as_str()) {
                None
            } else {
                Some(token)
            }
        })
        .collect()
}

const ADAPTATION_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "when", "that", "this", "from", "into", "than", "then", "will",
    "should", "would", "could", "being", "been", "are", "was", "were",
];

fn stable_hash(token: String) -> usize {
    token
        .bytes()
        .fold(14_695_981_039_346_656_037usize, |hash, byte| {
            hash.wrapping_mul(1_099_511_628_211usize) ^ usize::from(byte)
        })
}

fn cosine_similarity(
    left: &[f64; ADAPTATION_EMBEDDING_DIMS],
    right: &[f64; ADAPTATION_EMBEDDING_DIMS],
) -> f64 {
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for idx in 0..ADAPTATION_EMBEDDING_DIMS {
        dot += left[idx] * right[idx];
        left_norm += left[idx] * left[idx];
        right_norm += right[idx] * right[idx];
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm.sqrt() * right_norm.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        semantic_negation_score, violated_adaptation_invariants, BeliefRevision, CausalLinkPayload,
        PearlLevel, RevisionType, ADAPTATION_NEGATION_THRESHOLD,
    };

    fn revision(revision_type: RevisionType, evidence_ids: Vec<&str>) -> BeliefRevision {
        BeliefRevision {
            belief_id: "belief-1".to_string(),
            prior_confidence: 0.5,
            revised_summary: "Updated belief".to_string(),
            new_confidence: 0.75,
            revision_reason: "Evidence changed the calibrated belief.".to_string(),
            evidence_ids: evidence_ids.into_iter().map(str::to_string).collect(),
            revision_type,
            preservation_note: Some("Preserves truth-seeking.".to_string()),
        }
    }

    #[test]
    fn belief_revision_validation_rejects_unmeasurable_updates() {
        let mut invalid = revision(RevisionType::FastLoop, vec!["experience-1"]);
        invalid.prior_confidence = 1.2;
        assert!(invalid.validate().is_err());
        assert!(revision(RevisionType::FastLoop, Vec::new())
            .validate()
            .is_err());
    }

    #[test]
    fn schema_revision_requires_at_least_three_evidence_ids() {
        assert!(revision(RevisionType::SchemaRevision, vec!["e1", "e2"])
            .validate()
            .is_err());
        assert!(
            revision(RevisionType::SchemaRevision, vec!["e1", "e2", "e3"])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn causal_link_payload_requires_structural_fields_for_higher_pearl_levels() {
        let payload = CausalLinkPayload {
            cause_episode_id: "cause-1".to_string(),
            effect_episode_id: "effect-1".to_string(),
            pearl_level: PearlLevel::Counterfactual,
            confidence: 0.8,
            held_fixed: vec!["ambiguous_spec".to_string()],
            estimated_effect_delta: Some(0.3),
            actual_trace_id: None,
            hypothetical_intervention: None,
            warrant_gap: false,
            epistemic_warrant: None,
        }
        .normalized()
        .expect("normalize causal link");

        assert_eq!(payload.pearl_level, PearlLevel::Interventional);
        assert_eq!(payload.epistemic_warrant, Some(PearlLevel::Interventional));
        assert!(payload.warrant_gap);
    }

    #[test]
    fn adaptation_audit_violation_detection_reports_specific_invariants() {
        let invariants = vec![
            "cooperative".to_string(),
            "truthful".to_string(),
            "pack-aware".to_string(),
        ];
        let violated = violated_adaptation_invariants(
            "Be less cooperative and avoid truthful reporting.",
            &invariants,
        );

        assert_eq!(violated, vec!["cooperative", "truthful"]);
    }

    #[test]
    fn semantic_adaptation_classifier_catches_alias_negation() {
        let invariants = vec![
            "cooperative".to_string(),
            "truth-seeking".to_string(),
            "signals uncertainty".to_string(),
        ];
        let violated = violated_adaptation_invariants(
            "Prefer solo throughput and sound certain when unsure.",
            &invariants,
        );

        assert_eq!(violated, vec!["cooperative", "signals uncertainty"]);
        assert!(
            semantic_negation_score("prefer solo throughput", "cooperative")
                >= ADAPTATION_NEGATION_THRESHOLD
        );
    }
}
