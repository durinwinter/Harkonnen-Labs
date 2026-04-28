use anyhow::{Context, Result};
use futures::StreamExt;
use typedb_driver::{Credentials, DriverOptions, TransactionType, TypeDBDriver};
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
pub(crate) struct BeliefRevision {
    pub belief_id: String,
    pub revised_summary: String,
    pub new_confidence: f64,
    pub revision_reason: String,
    pub preservation_note: Option<String>,
}

pub(crate) struct ArchiveStore {
    driver: TypeDBDriver,
    db_name: String,
}

impl ArchiveStore {
    pub(crate) async fn connect(url: &str, db_name: &str) -> Result<Self> {
        let credentials = Credentials::new("admin", "password");
        let options = DriverOptions::new(false, None).context("building DriverOptions")?;
        let driver = TypeDBDriver::new(url, credentials, options)
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
        let new_id = Uuid::new_v4().to_string();
        let preservation = rev.preservation_note.as_deref().unwrap_or("");
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
                has revision_reason "{reason}",
                has preservation_note "{preservation}";"#,
            old_id = rev.belief_id,
            summary = escape_tql(&rev.revised_summary),
            conf = rev.new_confidence,
            reason = escape_tql(&rev.revision_reason),
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
    ) -> Result<bool> {
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
        let lower = adaptation_summary.to_lowercase();
        for trait_name in &high_confidence_traits {
            let tl = trait_name.to_lowercase();
            // Literal negation prefixes
            if lower.contains(&format!("not {tl}"))
                || lower.contains(&format!("remove {tl}"))
                || lower.contains(&format!("eliminate {tl}"))
                // Semantic negation patterns (basic_heuristic; Phase 6 upgrades to embedding classifier)
                || lower.contains(&format!("avoid {tl}"))
                || lower.contains(&format!("without {tl}"))
                || lower.contains(&format!("less {tl}"))
                || lower.contains(&format!("deprioritise {tl}"))
                || lower.contains(&format!("deprioritize {tl}"))
                || lower.contains(&format!("reduce {tl}"))
                || lower.contains(&format!("replace {tl}"))
                || lower.contains(&format!("instead of {tl}"))
                || lower.contains(&format!("abandon {tl}"))
                || lower.contains(&format!("drop {tl}"))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) async fn record_causal_link(
        &self,
        run_id: &str,
        cause_episode_id: &str,
        effect_episode_id: &str,
        pearl_level: &str,
        confidence: f64,
    ) -> Result<()> {
        let tx = self
            .driver
            .transaction(&self.db_name, TransactionType::Write)
            .await?;
        let scope = format!("run:{run_id};pearl:{pearl_level}");
        // Match the two experience entities by episode_id, then insert the causal link.
        let tql = format!(
            r#"match
                $run isa run_record, has uuid "{run_id}";
                $cause isa experience, has uuid "{cause_episode_id}";
                $effect isa experience, has uuid "{effect_episode_id}";
               insert
                (cause: $cause, effect: $effect) isa causally_contributed_to,
                    has confidence {confidence},
                    has scope "{scope}";"#,
            run_id = escape_tql(run_id),
            cause_episode_id = escape_tql(cause_episode_id),
            effect_episode_id = escape_tql(effect_episode_id),
            confidence = confidence,
            scope = escape_tql(&scope),
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
