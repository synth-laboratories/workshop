use super::{models::*, validation};
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

#[derive(Clone)]
pub struct HumanAnnotationService {
    db: Arc<crate::storage::Database>,
    store: crate::storage::content_store::ContentStore,
}

impl HumanAnnotationService {
    pub fn new(db: Arc<crate::storage::Database>, content_root: std::path::PathBuf) -> Self {
        Self {
            db,
            store: crate::storage::content_store::ContentStore::new(content_root),
        }
    }

    pub async fn preview(
        &self,
        request: HumanAnnotationPreviewRequest,
    ) -> Result<HumanAnnotationTaskPreview> {
        let task = request.task.0;
        validation::validate_task(&task)?;
        quiz_key_rows(&task, request.sealed_answer_keys.map(|value| value.0))?;
        let task_json = canonical_json(&task)?;
        let task_digest = sha256(task_json.as_bytes());
        let reviewer = request
            .reviewer_id
            .unwrap_or_else(|| "preview-reviewer".into());
        let questions = task["questions"].as_array().context("questions required")?;
        let evidence = task
            .get("evidence")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(HumanAnnotationTaskPreview {
            schema_version: PREVIEW_SCHEMA.into(),
            task_digest: task_digest.clone(),
            title: task["title"].as_str().unwrap_or("Review").into(),
            question_count: i32::try_from(questions.len()).unwrap_or(i32::MAX),
            required_question_count: i32::try_from(
                questions
                    .iter()
                    .filter(|question| question["required"].as_bool() == Some(true))
                    .count(),
            )
            .unwrap_or(i32::MAX),
            evidence_count: i32::try_from(evidence.len() + 1).unwrap_or(i32::MAX),
            evidence: crate::contract::specta::OpaqueJson(Value::Array(evidence)),
            presentation: crate::contract::specta::OpaqueJson(build_presentation(
                &task,
                &task_digest,
                &reviewer,
            )?),
            warnings: validation::evidence_preview_warnings(&task),
        })
    }

    pub async fn create(
        &self,
        request: HumanAnnotationCreateRequest,
    ) -> Result<HumanAnnotationTaskRef> {
        let task = request.task.0;
        validation::validate_task(&task)?;
        let quiz_keys = quiz_key_rows(&task, request.sealed_answer_keys.map(|value| value.0))?;
        let task_json = canonical_json(&task)?;
        let task_digest = sha256(task_json.as_bytes());
        let task_id = format!("hat_{}", Uuid::new_v4().simple());
        let session_id = format!("has_{}", Uuid::new_v4().simple());
        let now = now();
        let reviewer = request
            .reviewer_id
            .unwrap_or_else(|| "local-reviewer".into());
        let created_by = request.created_by.unwrap_or_else(|| "agent".into());
        let key = request.idempotency_key;
        let subject = task["subject"].clone();
        let rubric = task["rubric"].clone();
        let campaign_id = task
            .get("campaignId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let presentation = build_presentation(&task, &task_digest, &reviewer)?;
        self.db.run_transaction(move |conn| {
            if let Some(existing) = conn.query_row(
                "SELECT t.task_id,t.task_digest,c.state FROM human_annotation_tasks t LEFT JOIN human_annotation_campaigns c ON c.campaign_id=t.campaign_id WHERE t.idempotency_key=?1",
                [&key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)),
            ).optional()? {
                if existing.1 != task_digest { bail!("idempotency key is already bound to a different task digest"); }
                if existing.2.as_deref()==Some("closed") {bail!("closed campaigns do not accept new reviewer assignments");}
                verify_quiz_keys(conn,&existing.0,&quiz_keys)?;
                let existing_session: Option<String> = conn.query_row(
                    "SELECT session_id FROM human_annotation_sessions WHERE task_id=?1 AND reviewer_id=?2",
                    params![existing.0, reviewer], |row| row.get(0),
                ).optional()?;
                if let Some(existing_session)=existing_session {
                    let state:String=conn.query_row("SELECT state FROM human_annotation_sessions WHERE session_id=?1",[&existing_session],|row|row.get(0))?;
                    return Ok(HumanAnnotationTaskRef { task_id: existing.0, session_id: existing_session, task_digest, state, created: false });
                }
                conn.execute(
                    "INSERT INTO human_annotation_sessions(session_id,task_id,reviewer_id,state,draft_revision,draft_json,presentation_json,started_at,updated_at) VALUES(?1,?2,?3,'assigned',0,'{}',?4,?5,?5)",
                    params![session_id,existing.0,reviewer,canonical_json(&presentation)?,now],
                )?;
                conn.execute("UPDATE human_annotation_tasks SET state='assigned' WHERE task_id=?1",[&existing.0])?;
                append_event(conn,&existing.0,Some(&session_id),"human_annotation.session_assigned",json!({"reviewerId":reviewer}))?;
                return Ok(HumanAnnotationTaskRef { task_id: existing.0, session_id, task_digest, state: "assigned".into(), created: true });
            }
            if subject.get("kind").and_then(Value::as_str)==Some("visual_revision") {
                let revision=subject.get("revision").and_then(Value::as_i64).context("visual_revision subjects require an exact revision")?;
                let content_digest:Option<String>=conn.query_row("SELECT content_digest FROM visual_revisions WHERE visual_id=?1 AND revision=?2",params![subject["id"].as_str(),revision],|row|row.get(0)).context("visual revision is not registered in Workshop")?;
                // Template-backed visuals may carry their immutable bytes in a
                // sealed artifact bundle rather than `content_digest`. In that
                // case the seal receipt is the revision authority: it binds the
                // exact visual id/revision to the compiled runtime, index, and
                // data digests. Never fall back to a mutable visual-level field.
                let actual = if let Some(content_digest) = content_digest.filter(|digest| !digest.trim().is_empty()) {
                    content_digest
                } else {
                    conn.query_row(
                        "SELECT receipt_digest FROM visual_seals WHERE visual_id=?1 AND visual_revision=?2 ORDER BY created_at DESC LIMIT 1",
                        params![subject["id"].as_str(), revision],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .context("visual revision has neither a content digest nor a sealed artifact receipt")?
                };
                if normalize_digest(&actual)!=normalize_digest(subject["digest"].as_str().unwrap_or_default()){bail!("visual revision digest does not match the registered evidence");}
            }
            if let Some(campaign_id) = campaign_id.as_deref() {
                let campaign_state:Option<String>=conn.query_row("SELECT state FROM human_annotation_campaigns WHERE campaign_id=?1",[campaign_id],|row|row.get(0)).optional()?;
                if campaign_state.as_deref()==Some("closed"){bail!("closed campaigns do not accept new annotation tasks");}
                let campaign_name = task.get("campaign").and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or(campaign_id);
                let benchmark_family = task.get("campaign").and_then(|v| v.get("benchmarkFamily")).and_then(Value::as_str);
                let dataset_split = task.get("campaign").and_then(|v| v.get("datasetSplit")).and_then(Value::as_str);
                let policy_json=canonical_json(&task.get("campaign").and_then(|v|v.get("policy")).cloned().unwrap_or_else(||json!({})))?;
                conn.execute(
                    "INSERT OR IGNORE INTO human_annotation_campaigns(campaign_id,schema_version,name,benchmark_family,dataset_split,policy_json,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'open',?7)",
                    params![campaign_id,CAMPAIGN_SCHEMA,campaign_name,benchmark_family,dataset_split,policy_json,now],
                )?;
            }
            conn.execute(
                "INSERT INTO human_annotation_tasks(task_id,idempotency_key,campaign_id,schema_version,task_digest,subject_kind,subject_id,subject_revision,subject_digest,rubric_id,rubric_digest,task_json,state,created_by,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'assigned',?13,?14)",
                params![task_id,key,campaign_id,TASK_SCHEMA,task_digest,subject["kind"].as_str(),subject["id"].as_str(),subject.get("revision").and_then(Value::as_i64),subject["digest"].as_str(),rubric["id"].as_str(),rubric["digest"].as_str(),task_json,created_by,now],
            )?;
            for (question_id,key_digest,key_json) in &quiz_keys {
                conn.execute("INSERT INTO human_annotation_quiz_keys(task_id,question_id,key_digest,key_json,grader_version,created_at) VALUES(?1,?2,?3,?4,'human-quiz-exact.v1',?5)",params![task_id,question_id,key_digest,key_json,now])?;
            }
            conn.execute(
                "INSERT INTO human_annotation_sessions(session_id,task_id,reviewer_id,state,draft_revision,draft_json,presentation_json,started_at,updated_at) VALUES(?1,?2,?3,'assigned',0,'{}',?4,?5,?5)",
                params![session_id,task_id,reviewer,canonical_json(&presentation)?,now],
            )?;
            append_event(conn, &task_id, Some(&session_id), "human_annotation.task_created", json!({"taskDigest":task_digest}))?;
            Ok(HumanAnnotationTaskRef { task_id, session_id, task_digest, state: "assigned".into(), created: true })
        }).await
    }

    pub async fn open(&self, session_id: String) -> Result<HumanAnnotationSessionView> {
        self.db
            .run(move |conn| load_session(conn, &session_id))
            .await
    }

    /// Agent-readable status. Draft answers, comments, and attachments are
    /// deliberately absent until a sealed result exists.
    pub async fn status(&self, id: String) -> Result<HumanAnnotationStatus> {
        self.db.run(move |conn| {
            conn.query_row(
                "SELECT t.task_id,s.session_id,t.task_digest,s.state,s.draft_revision,r.result_id,r.result_digest,z.seal_digest,s.updated_at FROM human_annotation_tasks t JOIN human_annotation_sessions s ON s.task_id=t.task_id LEFT JOIN human_annotation_results r ON r.session_id=s.session_id LEFT JOIN human_annotation_result_seals z ON z.result_id=r.result_id WHERE t.task_id=?1 OR s.session_id=?1 OR r.result_id=?1 ORDER BY r.result_revision DESC LIMIT 1",
                [&id],
                |row| Ok(HumanAnnotationStatus { task_id: row.get(0)?, session_id: row.get(1)?, task_digest: row.get(2)?, state: row.get(3)?, revision: row.get(4)?, result_id: row.get(5)?, result_digest: row.get(6)?, seal_digest: row.get(7)?, updated_at: row.get(8)? }),
            ).map_err(Into::into)
        }).await
    }

    /// Full agent-readable record for a sealed result. Draft sessions never reach
    /// this path, and durable audio bytes remain in CAS rather than the response.
    pub async fn sealed_result(&self, result_id: String) -> Result<Value> {
        self.db.run(move |conn| {
			let (result_json, manifest_json, state, result_digest, seal_digest): (String, String, String, String, String) = conn.query_row(
				"SELECT r.result_json,z.manifest_json,r.state,r.result_digest,z.seal_digest FROM human_annotation_results r JOIN human_annotation_result_seals z ON z.result_id=r.result_id WHERE r.result_id=?1 AND r.state IN ('submitted','superseded')",
				[&result_id],
				|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
			).context("sealed human annotation result not found")?;
			Ok(json!({
				"schemaVersion": "synth.human-annotation-record.v1",
				"resultId": result_id,
				"state": state,
				"resultDigest": result_digest,
				"sealDigest": seal_digest,
				"result": serde_json::from_str::<Value>(&result_json)?,
				"seal": serde_json::from_str::<Value>(&manifest_json)?,
			}))
		}).await
    }

    pub async fn answer(
        &self,
        request: HumanAnnotationAnswerRequest,
    ) -> Result<HumanAnnotationMutationReceipt> {
        let answer = request.answer.0;
        self.db.run_transaction(move |conn| {
            let (task_id, revision, state, task_json): (String, i32, String, String) = conn.query_row(
                "SELECT s.task_id,s.draft_revision,s.state,t.task_json FROM human_annotation_sessions s JOIN human_annotation_tasks t ON t.task_id=s.task_id WHERE s.session_id=?1",
                [&request.session_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
            )?;
            ensure_mutable(&state, revision, request.expected_revision)?;
            let task: Value = serde_json::from_str(&task_json)?;
            let question = task["questions"].as_array().and_then(|items| items.iter().find(|item| item["id"] == request.question_id)).context("unknown question id")?;
            validation::validate_answer(question, &answer)?;
            let now = now();
            conn.execute("DELETE FROM human_annotation_answers WHERE session_id=?1 AND question_id=?2 AND result_id IS NULL", params![request.session_id,request.question_id])?;
            conn.execute("INSERT INTO human_annotation_answers(answer_id,session_id,question_id,question_revision,state,answer_json,answered_at,created_at) VALUES(?1,?2,?3,1,'answered',?4,?5,?5)", params![format!("haa_{}",Uuid::new_v4().simple()),request.session_id,request.question_id,canonical_json(&answer)?,now])?;
            bump(conn, &request.session_id, revision, "in_progress", &now)?;
            append_event(conn,&task_id,Some(&request.session_id),"human_annotation.answer_saved",json!({"questionId":request.question_id,"draftRevision":revision+1}))?;
            Ok(HumanAnnotationMutationReceipt{session_id:request.session_id,draft_revision:revision+1,state:"in_progress".into(),updated_at:now})
        }).await
    }

    pub async fn clear_answer(
        &self,
        session_id: String,
        expected_revision: i32,
        question_id: String,
    ) -> Result<HumanAnnotationMutationReceipt> {
        self.db.run_transaction(move |conn| {
            let (task_id,revision,state):(String,i32,String)=conn.query_row("SELECT task_id,draft_revision,state FROM human_annotation_sessions WHERE session_id=?1",[&session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            ensure_mutable(&state,revision,expected_revision)?;
            conn.execute("DELETE FROM human_annotation_answers WHERE session_id=?1 AND question_id=?2 AND result_id IS NULL",params![session_id,question_id])?;
            let timestamp=now(); bump(conn,&session_id,revision,"in_progress",&timestamp)?;
            append_event(conn,&task_id,Some(&session_id),"human_annotation.answer_cleared",json!({"questionId":question_id,"draftRevision":revision+1}))?;
            Ok(HumanAnnotationMutationReceipt{session_id,draft_revision:revision+1,state:"in_progress".into(),updated_at:timestamp})
        }).await
    }

    pub async fn comment(
        &self,
        request: HumanAnnotationCommentRequest,
    ) -> Result<HumanAnnotationMutationReceipt> {
        validation::validate_selector(&request.selector.0)?;
        if request.body_text.as_deref().unwrap_or("").trim().is_empty()
            && request.audio_attachment_id.is_none()
        {
            bail!("a comment needs text or durable audio");
        }
        self.db.run_transaction(move |conn| {
            let (task_id,revision,state,task_json):(String,i32,String,String)=conn.query_row("SELECT s.task_id,s.draft_revision,s.state,t.task_json FROM human_annotation_sessions s JOIN human_annotation_tasks t ON t.task_id=s.task_id WHERE s.session_id=?1",[&request.session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            ensure_mutable(&state,revision,request.expected_revision)?;
            let task:Value=serde_json::from_str(&task_json)?;
            let evidence=find_evidence_by_digest(&task,&request.evidence_digest).context("comment evidence digest is not bound to this task")?;
            validation::validate_selector_for_evidence(&request.selector.0,evidence)?;
            if let Some(id)=request.audio_attachment_id.as_deref() {
                let attachment_state:String=conn.query_row("SELECT state FROM human_annotation_attachments WHERE attachment_id=?1 AND session_id=?2",params![id,request.session_id],|r|r.get(0)).context("audio attachment does not belong to this session")?;
                if attachment_state!="saved" && attachment_state!="transcript_ready" { bail!("audio must be durably saved before it can be attached"); }
            }
            let timestamp=now();
            conn.execute("INSERT INTO human_annotation_comments(comment_id,session_id,evidence_digest,selector_json,body_text,audio_attachment_id,metadata_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'{}',?7,?7)",params![format!("hacm_{}",Uuid::new_v4().simple()),request.session_id,request.evidence_digest,canonical_json(&request.selector.0)?,request.body_text,request.audio_attachment_id,timestamp])?;
            bump(conn,&request.session_id,revision,"in_progress",&timestamp)?;
            append_event(conn,&task_id,Some(&request.session_id),"human_annotation.comment_saved",json!({"draftRevision":revision+1}))?;
            Ok(HumanAnnotationMutationReceipt{session_id:request.session_id,draft_revision:revision+1,state:"in_progress".into(),updated_at:timestamp})
        }).await
    }

    pub async fn audio_begin(
        &self,
        request: HumanAnnotationAudioBeginRequest,
    ) -> Result<HumanAnnotationAudioReceipt> {
        if !matches!(
            request.media_type.as_str(),
            "audio/webm" | "audio/mp4" | "audio/wav" | "audio/mpeg" | "audio/ogg"
        ) {
            bail!("unsupported annotation audio media type");
        }
        let id = format!("haat_{}", Uuid::new_v4().simple());
        let timestamp = now();
        self.db.run_transaction({let id=id.clone();move|conn|{
            let state:String=conn.query_row("SELECT state FROM human_annotation_sessions WHERE session_id=?1",[&request.session_id],|r|r.get(0))?;
            if state=="submitted" {bail!("submitted sessions are immutable");}
            conn.execute("INSERT INTO human_annotation_attachments(attachment_id,session_id,kind,state,media_type,metadata_json,created_at,updated_at) VALUES(?1,?2,'audio','recording',?3,?4,?5,?5)",params![id,request.session_id,request.media_type,canonical_json(&request.metadata.map(|v|v.0).unwrap_or(json!({})))?,timestamp])?;
            Ok(())
        }}).await?;
        Ok(HumanAnnotationAudioReceipt {
            attachment_id: id,
            state: "recording".into(),
            cas_digest: None,
            byte_size: None,
        })
    }

    pub async fn audio_append(
        &self,
        request: HumanAnnotationAudioChunkRequest,
    ) -> Result<HumanAnnotationAudioReceipt> {
        if request.chunk_index < 0 {
            bail!("audio chunk index must be non-negative");
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(request.base64_data)
            .context("invalid audio base64")?;
        if bytes.len() > 2 * 1024 * 1024 {
            bail!("audio chunk exceeds 2 MiB");
        }
        self.db.run_transaction(move|conn|{
            let state:String=conn.query_row("SELECT state FROM human_annotation_attachments WHERE attachment_id=?1",[&request.attachment_id],|r|r.get(0))?;
            if state!="recording" {bail!("audio attachment is not recording");}
            let persisted:i64=conn.query_row("SELECT COALESCE(SUM(length(bytes)),0) FROM human_annotation_audio_chunks WHERE attachment_id=?1 AND chunk_index!=?2",params![request.attachment_id,request.chunk_index],|r|r.get(0))?;
            if persisted.saturating_add(bytes.len() as i64)>100*1024*1024 {bail!("audio recording exceeds 100 MiB");}
            conn.execute("INSERT OR REPLACE INTO human_annotation_audio_chunks(attachment_id,chunk_index,bytes,created_at) VALUES(?1,?2,?3,?4)",params![request.attachment_id,request.chunk_index,bytes,now()])?;
            Ok(HumanAnnotationAudioReceipt{attachment_id:request.attachment_id,state:"recording".into(),cas_digest:None,byte_size:None})
        }).await
    }

    pub async fn audio_finish(
        &self,
        request: HumanAnnotationAudioFinishRequest,
    ) -> Result<HumanAnnotationAudioReceipt> {
        let attachment_id = request.attachment_id.clone();
        let (bytes,session_id)=self.db.run(move|conn|{
            let session_id:String=conn.query_row("SELECT session_id FROM human_annotation_attachments WHERE attachment_id=?1 AND state='recording'",[&attachment_id],|r|r.get(0))?;
            let mut statement=conn.prepare("SELECT bytes FROM human_annotation_audio_chunks WHERE attachment_id=?1 ORDER BY chunk_index")?;
            let chunks=statement.query_map([&attachment_id],|r|r.get::<_,Vec<u8>>(0))?;
            let mut bytes=Vec::new(); for chunk in chunks {bytes.extend(chunk?);} if bytes.is_empty(){bail!("audio recording has no persisted chunks");}
            Ok((bytes,session_id))
        }).await?;
        let digest = self.store.put_bytes("human_annotation_audio", &bytes)?;
        let size = i32::try_from(bytes.len()).context("audio too large")?;
        let timestamp = now();
        let id = request.attachment_id.clone();
        self.db.run_transaction(move|conn|{
            conn.execute("UPDATE human_annotation_attachments SET state='saved',cas_digest=?2,byte_size=?3,duration_ms=?4,updated_at=?5 WHERE attachment_id=?1 AND state='recording'",params![id,digest,size,request.duration_ms,timestamp])?;
            conn.execute("DELETE FROM human_annotation_audio_chunks WHERE attachment_id=?1",[&id])?;
            let task_id:String=conn.query_row("SELECT task_id FROM human_annotation_sessions WHERE session_id=?1",[&session_id],|r|r.get(0))?;
            append_event(conn,&task_id,Some(&session_id),"human_annotation.audio_saved",json!({"attachmentId":id,"casDigest":digest,"byteSize":size}))?;
            Ok(HumanAnnotationAudioReceipt{attachment_id:id,state:"saved".into(),cas_digest:Some(digest),byte_size:Some(size)})
        }).await
    }

    pub async fn audio_source(
        &self,
        session_id: String,
        attachment_id: String,
    ) -> Result<(Vec<u8>, String)> {
        let (digest, media_type) = self.db.run(move |conn| {
            conn.query_row(
                "SELECT cas_digest,media_type FROM human_annotation_attachments WHERE attachment_id=?1 AND session_id=?2 AND state IN ('saved','transcript_ready')",
                params![attachment_id,session_id],
                |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)),
            ).map_err(Into::into)
        }).await?;
        Ok((
            self.store.get_bytes("human_annotation_audio", &digest)?,
            media_type,
        ))
    }

    pub async fn save_machine_transcript(
        &self,
        request: HumanAnnotationAudioTranscribeRequest,
        transcript: String,
        model: String,
        language: String,
    ) -> Result<HumanAnnotationMutationReceipt> {
        let transcript_digest = sha256(transcript.as_bytes());
        self.db.run_transaction(move |conn| {
            let (task_id,revision,state):(String,i32,String)=conn.query_row("SELECT task_id,draft_revision,state FROM human_annotation_sessions WHERE session_id=?1",[&request.session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            ensure_mutable(&state,revision,request.expected_revision)?;
            let changed=conn.execute("UPDATE human_annotation_attachments SET state='transcript_ready',machine_transcript=?3,transcript_digest=?4,transcript_model=?5,transcript_language=?6,updated_at=?7 WHERE attachment_id=?1 AND session_id=?2 AND state IN ('saved','transcript_ready')",params![request.attachment_id,request.session_id,transcript,transcript_digest,model,language,now()])?;
            if changed!=1 {bail!("saved audio attachment does not belong to this session");}
            let timestamp=now(); bump(conn,&request.session_id,revision,"in_progress",&timestamp)?;
            append_event(conn,&task_id,Some(&request.session_id),"human_annotation.transcript_saved",json!({"attachmentId":request.attachment_id,"transcriptDigest":transcript_digest,"draftRevision":revision+1}))?;
            Ok(HumanAnnotationMutationReceipt{session_id:request.session_id,draft_revision:revision+1,state:"in_progress".into(),updated_at:timestamp})
        }).await
    }

    pub async fn correct_transcript(
        &self,
        request: HumanAnnotationTranscriptRequest,
    ) -> Result<HumanAnnotationMutationReceipt> {
        self.db.run_transaction(move|conn|{
            let (task_id,revision,state):(String,i32,String)=conn.query_row("SELECT task_id,draft_revision,state FROM human_annotation_sessions WHERE session_id=?1",[&request.session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            ensure_mutable(&state,revision,request.expected_revision)?;
            let changed=conn.execute("UPDATE human_annotation_comments SET corrected_transcript=?3,updated_at=?4 WHERE session_id=?1 AND audio_attachment_id=?2 AND result_id IS NULL",params![request.session_id,request.attachment_id,request.corrected_text,now()])?;
            if changed==0 {bail!("no draft comment is bound to that audio attachment");}
            let timestamp=now();bump(conn,&request.session_id,revision,"in_progress",&timestamp)?;
            append_event(conn,&task_id,Some(&request.session_id),"human_annotation.transcript_corrected",json!({"attachmentId":request.attachment_id,"draftRevision":revision+1}))?;
            Ok(HumanAnnotationMutationReceipt{session_id:request.session_id,draft_revision:revision+1,state:"in_progress".into(),updated_at:timestamp})
        }).await
    }

    pub async fn submit(
        &self,
        request: HumanAnnotationSubmitRequest,
    ) -> Result<HumanAnnotationResultReceipt> {
        self.db.run_transaction(move|conn|{
            let view=load_session(conn,&request.session_id)?;
            ensure_mutable(&view.state,view.draft_revision,request.expected_revision)?;
            let task=&view.task.0; let answers=&view.answers.0;
            let questions = task["questions"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let missing:Vec<String>=questions.iter().filter(|q|is_presented(q,answers)).filter(|q|q.get("required").and_then(Value::as_bool)==Some(true)).filter_map(|q|q["id"].as_str()).filter(|id|answers.get(*id).is_none()).map(str::to_owned).collect();
            if !missing.is_empty(){bail!("required questions are incomplete: {}",missing.join(", "));}
            if view.comments.is_empty(){bail!("at least one evidence-targeted comment is required");}
            let mut result_answers=Map::new();
            let mut quiz_grades=Vec::new();
            for question in questions {
                let Some(id)=question.get("id").and_then(Value::as_str) else {continue};
                if is_presented(question,answers) {
                    if let Some(answer)=answers.get(id){result_answers.insert(id.into(),answer.clone());}
                } else { result_answers.insert(id.into(),json!({"state":"not_presented"})); }
                if question.get("type").and_then(Value::as_str).is_some_and(|kind|kind.starts_with("quiz_")) {
                    quiz_grades.push(grade_quiz(conn,&view.task_id,question,answers.get(id))?);
                }
            }
            let draft_json:String=conn.query_row("SELECT draft_json FROM human_annotation_sessions WHERE session_id=?1",[&view.session_id],|row|row.get(0))?;
            let draft:Value=serde_json::from_str(&draft_json)?;
            let supersedes=draft.get("supersedesResultId").and_then(Value::as_str).map(str::to_owned);
            let result_revision=if let Some(source)=supersedes.as_deref(){
                conn.query_row("SELECT result_revision+1 FROM human_annotation_results WHERE result_id=?1 AND state='submitted'",[source],|row|row.get(0)).context("superseded source result is no longer current")?
            }else{1};
            let result_id=format!("har_{}",Uuid::new_v4().simple()); let timestamp=now();
            let result=json!({"schemaVersion":RESULT_SCHEMA,"resultId":result_id,"taskId":view.task_id,"sessionId":view.session_id,"taskDigest":view.task_digest,"subject":task["subject"],"rubric":task["rubric"],"answers":result_answers,"quizGrades":quiz_grades,"presentation":view.presentation.0,"comments":view.comments,"attachments":view.attachments,"supersedesResultId":supersedes,"submittedAt":timestamp});
            let result_json=canonical_json(&result)?; let result_digest=sha256(result_json.as_bytes());
            let manifest=json!({"schemaVersion":SEAL_SCHEMA,"resultId":result_id,"resultDigest":result_digest,"taskDigest":view.task_digest,"createdAt":timestamp});
            let manifest_json=canonical_json(&manifest)?; let seal_digest=sha256(manifest_json.as_bytes());
            conn.execute("INSERT INTO human_annotation_results(result_id,task_id,session_id,result_revision,result_digest,state,result_json,supersedes_id,created_at) VALUES(?1,?2,?3,?4,?5,'submitted',?6,?7,?8)",params![result_id,view.task_id,view.session_id,result_revision,result_digest,result_json,supersedes,timestamp])?;
            if let Some(source)=supersedes.as_deref(){conn.execute("UPDATE human_annotation_results SET state='superseded' WHERE result_id=?1 AND state='submitted'",[source])?;}
            conn.execute("UPDATE human_annotation_answers SET result_id=?2 WHERE session_id=?1 AND result_id IS NULL",params![view.session_id,result_id])?;
            conn.execute("UPDATE human_annotation_comments SET result_id=?2 WHERE session_id=?1 AND result_id IS NULL",params![view.session_id,result_id])?;
            conn.execute("INSERT INTO human_annotation_result_seals(result_id,schema_version,seal_digest,manifest_json,created_at) VALUES(?1,?2,?3,?4,?5)",params![result_id,SEAL_SCHEMA,seal_digest,manifest_json,timestamp])?;
            conn.execute("UPDATE human_annotation_sessions SET state='submitted',submitted_at=?2,updated_at=?2 WHERE session_id=?1",params![view.session_id,timestamp])?;
            conn.execute("UPDATE human_annotation_tasks SET state='submitted' WHERE task_id=?1",[&view.task_id])?;
            append_event(conn,&view.task_id,Some(&view.session_id),"human_annotation.submitted",json!({"resultId":result_id,"resultDigest":result_digest,"sealDigest":seal_digest}))?;
            Ok(HumanAnnotationResultReceipt{task_id:view.task_id,session_id:view.session_id,result_id,result_digest,seal_digest,state:"submitted".into(),submitted_at:timestamp})
        }).await
    }

    pub async fn list(&self, query: HumanAnnotationListQuery) -> Result<Vec<Value>> {
        self.db.run(move|conn|{
            let mut sql="SELECT t.task_id,t.campaign_id,t.task_digest,s.state,t.created_at,s.session_id FROM human_annotation_tasks t JOIN human_annotation_sessions s ON s.task_id=t.task_id WHERE 1=1".to_string();
            let mut values:Vec<String>=Vec::new();
            if let Some(id)=query.campaign_id {sql.push_str(" AND t.campaign_id=?");values.push(id);}
            if let Some(state)=query.state {sql.push_str(" AND s.state=?");values.push(state);}
            sql.push_str(" ORDER BY t.created_at DESC LIMIT ?"); values.push(query.limit.unwrap_or(100).clamp(1,500).to_string());
            let mut statement=conn.prepare(&sql)?; let params=rusqlite::params_from_iter(values.iter());
            let rows=statement.query_map(params,|r|Ok(json!({"taskId":r.get::<_,String>(0)?,"campaignId":r.get::<_,Option<String>>(1)?,"taskDigest":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"createdAt":r.get::<_,String>(4)?,"sessionId":r.get::<_,String>(5)?})))?;
            rows.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)
        }).await
    }

    pub async fn cancel(
        &self,
        request: HumanAnnotationCancelRequest,
    ) -> Result<HumanAnnotationStatus> {
        if request.reason.trim().is_empty() {
            bail!("cancellation reason is required");
        }
        let task_id = request.task_id.clone();
        self.db.run_transaction(move|conn|{
            let submitted:i64=conn.query_row("SELECT COUNT(*) FROM human_annotation_results WHERE task_id=?1 AND state='submitted'",[&request.task_id],|r|r.get(0))?;
            if submitted>0 {bail!("submitted human annotation work cannot be cancelled");}
            let timestamp=now();
            let changed=conn.execute("UPDATE human_annotation_tasks SET state='cancelled',cancelled_at=?2,cancellation_reason=?3 WHERE task_id=?1 AND state!='cancelled'",params![request.task_id,timestamp,request.reason])?;
            if changed==0 { let exists:i64=conn.query_row("SELECT COUNT(*) FROM human_annotation_tasks WHERE task_id=?1",[&request.task_id],|r|r.get(0))?; if exists==0 {bail!("human annotation task not found");} }
            conn.execute("UPDATE human_annotation_sessions SET state='cancelled',updated_at=?2 WHERE task_id=?1 AND state!='submitted'",params![request.task_id,timestamp])?;
            append_event(conn,&request.task_id,None,"human_annotation.cancelled",json!({"reason":request.reason}))?;
            Ok(())
        }).await?;
        self.status(task_id).await
    }

    pub async fn export(
        &self,
        request: HumanAnnotationExportRequest,
    ) -> Result<HumanAnnotationExportReceipt> {
        if !matches!(request.format.as_str(), "json" | "jsonl") {
            bail!("human annotation export format must be json or jsonl");
        }
        let result_id = request.result_id.clone();
        let format = request.format.clone();
        let bundle=self.db.run(move|conn|{
            let (result,seal):(String,String)=conn.query_row("SELECT r.result_json,z.manifest_json FROM human_annotation_results r JOIN human_annotation_result_seals z ON z.result_id=r.result_id WHERE r.result_id=?1 AND r.state IN ('submitted','superseded')",[&result_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let value=json!({"schemaVersion":"synth.human-annotation-export.v1","result":serde_json::from_str::<Value>(&result)?,"seal":serde_json::from_str::<Value>(&seal)?});
            let mut encoded=canonical_json(&value)?;
            if format=="jsonl" { encoded.push('\n'); }
            Ok(encoded.into_bytes())
        }).await?;
        let export_digest = self.store.put_bytes("human_annotation_exports", &bundle)?;
        Ok(HumanAnnotationExportReceipt {
            result_id: request.result_id,
            format: request.format,
            export_digest,
            byte_size: i32::try_from(bundle.len()).context("export too large")?,
        })
    }

    pub async fn submitted_for_subject(
        &self,
        subject_kind: String,
        subject_id: String,
    ) -> Result<Vec<Value>> {
        self.db.run(move |conn| {
            let mut statement=conn.prepare("SELECT r.result_json,z.seal_digest FROM human_annotation_tasks t JOIN human_annotation_results r ON r.task_id=t.task_id AND r.state='submitted' JOIN human_annotation_result_seals z ON z.result_id=r.result_id WHERE t.subject_kind=?1 AND t.subject_id=?2 ORDER BY r.created_at")?;
            let rows=statement.query_map(params![subject_kind,subject_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?;
            let mut out=Vec::new();
            for row in rows {
                let (raw,seal)=row?;
                let mut value:Value=serde_json::from_str(&raw)?;
                value.as_object_mut().context("stored human annotation result must be an object")?.insert("sealDigest".into(),json!(seal));
                out.push(value);
            }
            Ok(out)
        }).await
    }

    pub async fn campaign_create(
        &self,
        request: HumanAnnotationCampaignCreateRequest,
    ) -> Result<Value> {
        let campaign = request.campaign.0;
        let campaign_id = campaign
            .get("campaignId")
            .or_else(|| campaign.get("id"))
            .and_then(Value::as_str)
            .context("campaignId required")?
            .trim()
            .to_owned();
        let name = campaign["name"]
            .as_str()
            .context("campaign.name required")?
            .trim()
            .to_owned();
        if campaign_id.is_empty() || name.is_empty() {
            bail!("campaign id and name must not be empty");
        }
        let benchmark = campaign
            .get("benchmarkFamily")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let split = campaign
            .get("datasetSplit")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let policy = campaign.get("policy").cloned().unwrap_or_else(|| json!({}));
        let policy_json = canonical_json(&policy)?;
        let id = campaign_id.clone();
        self.db.run_transaction(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO human_annotation_campaigns(campaign_id,schema_version,name,benchmark_family,dataset_split,policy_json,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'open',?7)",
                params![campaign_id,CAMPAIGN_SCHEMA,name,benchmark,split,policy_json,now()],
            )?;
            campaign_projection(conn,&id)
        }).await
    }

    pub async fn campaign_status(&self, campaign_id: String) -> Result<Value> {
        self.db
            .run(move |conn| campaign_projection(conn, &campaign_id))
            .await
    }

    pub async fn campaign_close(
        &self,
        request: HumanAnnotationCampaignActionRequest,
    ) -> Result<Value> {
        if request.rationale.trim().is_empty() {
            bail!("campaign close rationale is required");
        }
        self.db.run_transaction(move |conn| {
            let projection=campaign_projection(conn,&request.campaign_id)?;
            let needs=projection["requiresAdjudication"].as_bool()==Some(true);
            let adjudications=projection["adjudicationCount"].as_i64().unwrap_or(0);
            let next=if needs && adjudications==0 {"needs_adjudication"} else {"closed"};
            let closed=if next=="closed" {Some(now())} else {None};
            let changed=conn.execute("UPDATE human_annotation_campaigns SET state=?2,closed_at=?3 WHERE campaign_id=?1 AND state!='closed'",params![request.campaign_id,next,closed])?;
            if changed==0 && projection["state"]!="closed" {bail!("campaign state could not be changed");}
            append_campaign_event(conn,&request.campaign_id,if next=="closed"{"human_annotation.campaign_closed"}else{"human_annotation.campaign_needs_adjudication"},json!({"rationale":request.rationale}))?;
            campaign_projection(conn,&request.campaign_id)
        }).await
    }

    pub async fn campaign_adjudicate(
        &self,
        request: HumanAnnotationCampaignAdjudicateRequest,
    ) -> Result<Value> {
        if request.result_ids.len() < 2 {
            bail!("adjudication requires at least two source results");
        }
        if request.rationale.trim().is_empty() {
            bail!("adjudication rationale is required");
        }
        let mut ids = request.result_ids.clone();
        ids.sort();
        ids.dedup();
        if ids.len() != request.result_ids.len() {
            bail!("adjudication source results must be unique");
        }
        let decision = request.decision.0;
        if !decision.is_object() {
            bail!("adjudication decision must be an object");
        }
        let adjudicator = request
            .adjudicator_id
            .unwrap_or_else(|| "local-adjudicator".into());
        self.db.run_transaction(move |conn| {
            for result_id in &ids {
                let belongs:i64=conn.query_row("SELECT COUNT(*) FROM human_annotation_results r JOIN human_annotation_tasks t ON t.task_id=r.task_id WHERE r.result_id=?1 AND t.campaign_id=?2",params![result_id,request.campaign_id],|row|row.get(0))?;
                if belongs!=1 {bail!("source result `{result_id}` is not in this campaign");}
            }
            let timestamp=now();
            let canonical=canonical_json(&json!({"campaignId":request.campaign_id,"sourceResultIds":ids,"decision":decision,"rationale":request.rationale,"adjudicatorId":adjudicator,"createdAt":timestamp}))?;
            let digest=sha256(canonical.as_bytes());
            conn.execute("INSERT INTO human_annotation_adjudications(adjudication_id,campaign_id,schema_version,source_result_ids_json,decision_json,rationale,adjudicator_id,decision_digest,created_at) VALUES(?1,?2,'synth.human-annotation-adjudication.v1',?3,?4,?5,?6,?7,?8)",params![format!("haadj_{}",Uuid::new_v4().simple()),request.campaign_id,canonical_json(&json!(ids))?,canonical_json(&decision)?,request.rationale,adjudicator,digest,timestamp])?;
            conn.execute("UPDATE human_annotation_campaigns SET state='adjudicated' WHERE campaign_id=?1 AND state!='closed'",[&request.campaign_id])?;
            append_campaign_event(conn,&request.campaign_id,"human_annotation.campaign_adjudicated",json!({"decisionDigest":digest}))?;
            campaign_projection(conn,&request.campaign_id)
        }).await
    }

    pub async fn supersede(
        &self,
        request: HumanAnnotationSupersedeRequest,
    ) -> Result<HumanAnnotationTaskRef> {
        if request.reason.trim().is_empty() {
            bail!("supersession reason is required");
        }
        self.db.run_transaction(move |conn| {
            let (task_id,task_digest,task_json,result_json,original_reviewer):(String,String,String,String,String)=conn.query_row(
                "SELECT t.task_id,t.task_digest,t.task_json,r.result_json,s.reviewer_id FROM human_annotation_results r JOIN human_annotation_tasks t ON t.task_id=r.task_id JOIN human_annotation_sessions s ON s.session_id=r.session_id WHERE r.result_id=?1 AND r.state='submitted'",
                [&request.result_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))
                .context("submitted result not found")?;
            let reviewer=request.reviewer_id.unwrap_or_else(||format!("{original_reviewer}:correction:{}",&Uuid::new_v4().simple().to_string()[..8]));
            let session_id=format!("has_{}",Uuid::new_v4().simple());
            let task:Value=serde_json::from_str(&task_json)?;
            let result:Value=serde_json::from_str(&result_json)?;
            let timestamp=now();
            conn.execute("INSERT INTO human_annotation_sessions(session_id,task_id,reviewer_id,state,draft_revision,draft_json,presentation_json,started_at,updated_at) VALUES(?1,?2,?3,'in_progress',0,?4,?5,?6,?6)",params![session_id,task_id,reviewer,canonical_json(&json!({"supersedesResultId":request.result_id,"reason":request.reason}))?,canonical_json(&build_presentation(&task,&task_digest,&reviewer)?)?,timestamp])?;
            if let Some(answers)=result.get("answers").and_then(Value::as_object){
                for (question_id,answer) in answers {
                    if answer.get("state").and_then(Value::as_str)==Some("not_presented"){continue;}
                    conn.execute("INSERT INTO human_annotation_answers(answer_id,session_id,question_id,question_revision,state,answer_json,answered_at,created_at) VALUES(?1,?2,?3,1,'answered',?4,?5,?5)",params![format!("haa_{}",Uuid::new_v4().simple()),session_id,question_id,canonical_json(answer)?,timestamp])?;
                }
            }
            append_event(conn,&task_id,Some(&session_id),"human_annotation.supersession_started",json!({"supersedesResultId":request.result_id,"reason":request.reason}))?;
            Ok(HumanAnnotationTaskRef{task_id,session_id,task_digest,state:"in_progress".into(),created:true})
        }).await
    }
}

fn find_evidence_by_digest<'a>(task: &'a Value, digest: &str) -> Option<&'a Value> {
    std::iter::once(task.get("subject"))
        .chain(
            task.get("evidence")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(Some),
        )
        .flatten()
        .find(|item| {
            item.get("digest")
                .and_then(Value::as_str)
                .is_some_and(|candidate| normalize_digest(candidate) == normalize_digest(digest))
        })
}

fn append_campaign_event(
    conn: &rusqlite::Connection,
    campaign_id: &str,
    kind: &str,
    payload: Value,
) -> Result<()> {
    let task_id: Option<String> = conn
        .query_row(
            "SELECT task_id FROM human_annotation_tasks WHERE campaign_id=?1 ORDER BY created_at LIMIT 1",
            [campaign_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(task_id) = task_id {
        append_event(conn, &task_id, None, kind, payload)?;
    }
    Ok(())
}

fn campaign_projection(conn: &rusqlite::Connection, campaign_id: &str) -> Result<Value> {
    let (name,benchmark,split,policy,state,created_at,closed_at):(String,Option<String>,Option<String>,String,String,String,Option<String>)=conn.query_row(
        "SELECT name,benchmark_family,dataset_split,policy_json,state,created_at,closed_at FROM human_annotation_campaigns WHERE campaign_id=?1",
        [campaign_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)))
        .context("human annotation campaign not found")?;
    let mut statement=conn.prepare("SELECT r.result_id,r.result_digest,r.state,r.result_json,z.seal_digest,s.reviewer_id FROM human_annotation_results r JOIN human_annotation_tasks t ON t.task_id=r.task_id JOIN human_annotation_sessions s ON s.session_id=r.session_id JOIN human_annotation_result_seals z ON z.result_id=r.result_id WHERE t.campaign_id=?1 ORDER BY r.created_at")?;
    let rows = statement.query_map([campaign_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut results = Vec::new();
    let mut answer_counts: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, i64>,
    > = std::collections::BTreeMap::new();
    for row in rows {
        let (result_id, result_digest, result_state, raw, seal_digest, reviewer_id) = row?;
        let result: Value = serde_json::from_str(&raw)?;
        if result_state == "submitted" {
            if let Some(answers) = result.get("answers").and_then(Value::as_object) {
                for (question_id, answer) in answers {
                    if answer.get("state").and_then(Value::as_str) == Some("not_presented") {
                        continue;
                    }
                    let value = answer.get("value").unwrap_or(answer);
                    *answer_counts
                        .entry(question_id.clone())
                        .or_default()
                        .entry(canonical_json(value)?)
                        .or_default() += 1;
                }
            }
        }
        results.push(json!({"resultId":result_id,"resultDigest":result_digest,"sealDigest":seal_digest,"state":result_state,"reviewerId":reviewer_id,"submittedAt":result["submittedAt"],"supersedesResultId":result["supersedesResultId"]}));
    }
    let agreement=answer_counts.into_iter().map(|(question_id,counts)|{
        let total:i64=counts.values().sum();
        let top=counts.values().copied().max().unwrap_or(0);
        json!({"questionId":question_id,"responseCount":total,"distinctAnswerCount":counts.len(),"agreement":if total>0{top as f64/total as f64}else{0.0},"counts":counts})
    }).collect::<Vec<_>>();
    let disagreement_count = agreement
        .iter()
        .filter(|item| item["distinctAnswerCount"].as_u64().unwrap_or(0) > 1)
        .count();
    let task_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM human_annotation_tasks WHERE campaign_id=?1",
        [campaign_id],
        |row| row.get(0),
    )?;
    let session_count:i64=conn.query_row("SELECT COUNT(*) FROM human_annotation_sessions s JOIN human_annotation_tasks t ON t.task_id=s.task_id WHERE t.campaign_id=?1",[campaign_id],|row|row.get(0))?;
    let submitted_count:i64=conn.query_row("SELECT COUNT(*) FROM human_annotation_results r JOIN human_annotation_tasks t ON t.task_id=r.task_id WHERE t.campaign_id=?1 AND r.state='submitted'",[campaign_id],|row|row.get(0))?;
    let mut adjudication_statement=conn.prepare("SELECT adjudication_id,source_result_ids_json,decision_json,rationale,adjudicator_id,decision_digest,created_at FROM human_annotation_adjudications WHERE campaign_id=?1 ORDER BY created_at")?;
    let adjudications=adjudication_statement.query_map([campaign_id],|row|Ok(json!({"adjudicationId":row.get::<_,String>(0)?,"sourceResultIds":serde_json::from_str::<Value>(&row.get::<_,String>(1)?).unwrap_or(Value::Null),"decision":serde_json::from_str::<Value>(&row.get::<_,String>(2)?).unwrap_or(Value::Null),"rationale":row.get::<_,String>(3)?,"adjudicatorId":row.get::<_,String>(4)?,"decisionDigest":row.get::<_,String>(5)?,"createdAt":row.get::<_,String>(6)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
    Ok(
        json!({"schemaVersion":CAMPAIGN_SCHEMA,"campaignId":campaign_id,"name":name,"benchmarkFamily":benchmark,"datasetSplit":split,"policy":serde_json::from_str::<Value>(&policy)?,"state":state,"createdAt":created_at,"closedAt":closed_at,"taskCount":task_count,"sessionCount":session_count,"submittedResultCount":submitted_count,"resultCount":results.len(),"disagreementCount":disagreement_count,"requiresAdjudication":disagreement_count>0,"agreement":agreement,"results":results,"adjudicationCount":adjudications.len(),"adjudications":adjudications}),
    )
}

fn load_session(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<HumanAnnotationSessionView> {
    let (task_id,task_digest,task_json,state,revision,presentation_json,updated_at):(String,String,String,String,i32,String,String)=conn.query_row("SELECT s.task_id,t.task_digest,t.task_json,s.state,s.draft_revision,s.presentation_json,s.updated_at FROM human_annotation_sessions s JOIN human_annotation_tasks t ON t.task_id=s.task_id WHERE s.session_id=?1",[session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    let mut answers = Map::new();
    let mut statement=conn.prepare("SELECT question_id,answer_json FROM human_annotation_answers WHERE session_id=?1 AND result_id IS NULL ORDER BY created_at")?;
    for row in statement.query_map([session_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (id, json) = row?;
        answers.insert(id, serde_json::from_str(&json)?);
    }
    let comments=query_json_rows(conn,"SELECT json_object('commentId',comment_id,'evidenceDigest',evidence_digest,'selector',json(selector_json),'bodyText',body_text,'audioAttachmentId',audio_attachment_id,'correctedTranscript',corrected_transcript) FROM human_annotation_comments WHERE session_id=?1 AND tombstoned=0 ORDER BY created_at",session_id)?;
    let attachments=query_json_rows(conn,"SELECT json_object('attachmentId',a.attachment_id,'kind',a.kind,'state',a.state,'mediaType',a.media_type,'casDigest',a.cas_digest,'byteSize',a.byte_size,'durationMs',a.duration_ms,'machineTranscript',a.machine_transcript,'correctedTranscript',(SELECT c.corrected_transcript FROM human_annotation_comments c WHERE c.audio_attachment_id=a.attachment_id AND c.tombstoned=0 ORDER BY c.created_at DESC LIMIT 1),'transcriptDigest',a.transcript_digest,'transcriptModel',a.transcript_model,'transcriptLanguage',a.transcript_language) FROM human_annotation_attachments a WHERE a.session_id=?1 ORDER BY a.created_at",session_id)?;
    let result=conn.query_row("SELECT result_json FROM human_annotation_results WHERE session_id=?1 AND state IN ('submitted','superseded') ORDER BY result_revision DESC LIMIT 1",[session_id],|r|r.get::<_,String>(0)).optional()?.map(|raw|serde_json::from_str(&raw)).transpose()?.map(crate::contract::specta::OpaqueJson);
    Ok(HumanAnnotationSessionView {
        schema_version: SESSION_SCHEMA.into(),
        task_id,
        session_id: session_id.into(),
        task_digest,
        task: crate::contract::specta::OpaqueJson(serde_json::from_str(&task_json)?),
        state,
        draft_revision: revision,
        answers: crate::contract::specta::OpaqueJson(Value::Object(answers)),
        presentation: crate::contract::specta::OpaqueJson(serde_json::from_str(
            &presentation_json,
        )?),
        comments: comments
            .into_iter()
            .map(crate::contract::specta::OpaqueJson)
            .collect(),
        attachments: attachments
            .into_iter()
            .map(crate::contract::specta::OpaqueJson)
            .collect(),
        result,
        updated_at,
    })
}
fn query_json_rows(conn: &rusqlite::Connection, sql: &str, id: &str) -> Result<Vec<Value>> {
    let mut s = conn.prepare(sql)?;
    let rows = s.query_map([id], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(serde_json::from_str(&row?)?);
    }
    Ok(out)
}
fn ensure_mutable(state: &str, actual: i32, expected: i32) -> Result<()> {
    if state == "submitted" {
        bail!("submitted sessions are immutable");
    }
    if actual != expected {
        bail!("draft revision conflict: expected {expected}, current {actual}");
    }
    Ok(())
}
fn bump(
    conn: &rusqlite::Connection,
    id: &str,
    revision: i32,
    state: &str,
    timestamp: &str,
) -> Result<()> {
    let changed=conn.execute("UPDATE human_annotation_sessions SET draft_revision=?2,state=?3,updated_at=?4 WHERE session_id=?1 AND draft_revision=?5",params![id,revision+1,state,timestamp,revision])?;
    if changed != 1 {
        bail!("draft revision conflict");
    }
    conn.execute(
        "UPDATE human_annotation_tasks SET state=?2 WHERE task_id=(SELECT task_id FROM human_annotation_sessions WHERE session_id=?1) AND state NOT IN ('submitted','cancelled')",
        params![id,state],
    )?;
    Ok(())
}
fn append_event(
    conn: &rusqlite::Connection,
    task_id: &str,
    session_id: Option<&str>,
    kind: &str,
    payload: Value,
) -> Result<()> {
    conn.execute("INSERT INTO human_annotation_events(event_id,task_id,session_id,kind,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![format!("haev_{}",Uuid::new_v4().simple()),task_id,session_id,kind,canonical_json(&payload)?,now()])?;
    Ok(())
}
fn canonical_json(value: &Value) -> Result<String> {
    serde_json::to_string(&canonical_value(value)).context("serialize canonical annotation JSON")
}
fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        other => other.clone(),
    }
}
fn build_presentation(task: &Value, task_digest: &str, reviewer: &str) -> Result<Value> {
    let mut orders = Map::new();
    for question in task["questions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if question
            .get("randomize")
            .or_else(|| question.get("randomizeOptions"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            continue;
        }
        let Some(id) = question.get("id").and_then(Value::as_str) else {
            continue;
        };
        let mut option_ids = question
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|o| o.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        option_ids.sort_by_key(|option| {
            sha256(format!("{task_digest}:{reviewer}:{id}:{option}").as_bytes())
        });
        orders.insert(id.into(), json!({"optionOrder":option_ids}));
    }
    Ok(
        json!({"questionOrder":task["questions"].as_array().map(|items|items.iter().filter_map(|q|q.get("id").and_then(Value::as_str)).collect::<Vec<_>>()).unwrap_or_default(),"questions":orders}),
    )
}
fn is_presented(question: &Value, answers: &Value) -> bool {
    let Some(when) = question.get("visibility").and_then(|v| v.get("when")) else {
        return true;
    };
    let Some(question_id) = when.get("questionId").and_then(Value::as_str) else {
        return true;
    };
    let answer = answers
        .get(question_id)
        .and_then(|v| v.get("value"))
        .or_else(|| answers.get(question_id));
    match when
        .get("operator")
        .and_then(Value::as_str)
        .unwrap_or("equals")
    {
        "answered" => answer.is_some(),
        "not_answered" => answer.is_none(),
        "equals" => answer == when.get("value"),
        "not_equals" => answer != when.get("value"),
        "contains" => answer
            .and_then(Value::as_array)
            .map(|a| a.contains(when.get("value").unwrap_or(&Value::Null)))
            .unwrap_or(false),
        "contains_any" => {
            let expected = when.get("value").and_then(Value::as_array);
            answer
                .and_then(Value::as_array)
                .zip(expected)
                .map(|(a, b)| a.iter().any(|v| b.contains(v)))
                .unwrap_or(false)
        }
        _ => false,
    }
}
fn quiz_key_rows(task: &Value, keys: Option<Value>) -> Result<Vec<(String, String, String)>> {
    let keys = keys.unwrap_or_else(|| json!({}));
    let object = keys
        .as_object()
        .context("sealedAnswerKeys must be an object keyed by question ID")?;
    let questions = task["questions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let quiz_ids = questions
        .iter()
        .filter(|q| {
            q.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("quiz_"))
        })
        .filter_map(|q| q.get("id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    for id in object.keys() {
        if !quiz_ids.contains(id.as_str()) {
            bail!("sealed answer key `{id}` does not name a quiz question");
        }
    }
    object
        .iter()
        .map(|(id, key)| {
            let key_json = canonical_json(key)?;
            Ok((id.clone(), sha256(key_json.as_bytes()), key_json))
        })
        .collect()
}
fn verify_quiz_keys(
    conn: &rusqlite::Connection,
    task_id: &str,
    keys: &[(String, String, String)],
) -> Result<()> {
    for (question_id, digest, _) in keys {
        let stored:Option<String>=conn.query_row("SELECT key_digest FROM human_annotation_quiz_keys WHERE task_id=?1 AND question_id=?2",params![task_id,question_id],|row|row.get(0)).optional()?;
        if stored.as_deref() != Some(digest) {
            bail!("sealed quiz key digest does not match the existing task");
        }
    }
    Ok(())
}
fn grade_quiz(
    conn: &rusqlite::Connection,
    task_id: &str,
    question: &Value,
    answer: Option<&Value>,
) -> Result<Value> {
    let id = question["id"].as_str().unwrap_or_default();
    let key:Option<(String,String,String)>=conn.query_row("SELECT key_json,key_digest,grader_version FROM human_annotation_quiz_keys WHERE task_id=?1 AND question_id=?2",params![task_id,id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    let Some((key_json, key_digest, grader_version)) = key else {
        return Ok(
            json!({"questionId":id,"outcome":"unavailable","score":Value::Null,"reason":"sealed answer key unavailable"}),
        );
    };
    let Some(answer) = answer else {
        return Ok(
            json!({"questionId":id,"outcome":"unanswered","score":Value::Null,"answerKeyDigest":key_digest,"graderVersion":grader_version}),
        );
    };
    let expected: Value = serde_json::from_str(&key_json)?;
    let actual = answer.get("value").unwrap_or(answer);
    let correct = if question.get("type").and_then(Value::as_str) == Some("quiz_multi") {
        let left = actual
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(canonical_json)
            .collect::<Result<BTreeSet<_>>>()?;
        let right = expected
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(canonical_json)
            .collect::<Result<BTreeSet<_>>>()?;
        left == right
    } else {
        actual == &expected
    };
    Ok(
        json!({"questionId":id,"outcome":if correct{"correct"}else{"incorrect"},"score":if correct{1.0}else{0.0},"answerKeyDigest":key_digest,"graderVersion":grader_version}),
    )
}
fn sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("sha256:{:x}", h.finalize())
}
fn normalize_digest(value: &str) -> &str {
    value.strip_prefix("sha256:").unwrap_or(value)
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Value {
        json!({
            "schemaVersion":TASK_SCHEMA,
            "campaignId":"campaign_test",
            "campaign":{"name":"Test campaign","benchmarkFamily":"VisualsBench","datasetSplit":"reference"},
            "title":"Review the evidence",
            "instructions":"Answer from the digest-bound evidence.",
            "subject":{"kind":"visual_revision","id":"vis_test","revision":1,"digest":format!("sha256:{}","a".repeat(64))},
            "rubric":{"id":"visualsbench.reference.v1","digest":format!("sha256:{}","b".repeat(64))},
            "questions":[
                {"id":"visible","type":"yes_no","prompt":"Would you trust this visual when choosing an outcome?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Consider the primary outcome and its context.","answerRule":"Answer yes only if you could decide without guessing."},"answerGuidance":{"yes":"I could decide without guessing.","no":"I would hesitate or need more context."},"required":true},
                {"id":"hidden","type":"short_text","prompt":"What single change would reduce your hesitation most?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Focus on the part that caused hesitation.","answerRule":"Name one change and why it matters."},"required":true,"visibility":{"when":{"questionId":"visible","operator":"equals","value":"no"}}},
                {"id":"preference","type":"preference","prompt":"Which candidate would you use to make the decision?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Consider both candidates in their initial state.","answerRule":"Choose the one you would act on with less hesitation."},"options":[{"id":"candidate_a"},{"id":"candidate_b"}],"randomize":true}
            ]
        })
    }

    fn service() -> (tempfile::TempDir, HumanAnnotationService) {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::open(dir.path()).unwrap();
        storage.database().with_conn(|conn| {
            let timestamp=now();
            let digest=format!("sha256:{}","a".repeat(64));
            conn.execute("INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,content_digest,metadata_json,created_at,updated_at) VALUES('vis_test',1,'Test','test.template','ready','react_app','{}',?1,'{}',?2,?2)",params![digest,timestamp])?;
            conn.execute("INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,content_digest,created_at) VALUES('vis_test',1,'test.template','react_app',?1,?2)",params![digest,timestamp])?;
            Ok(())
        }).unwrap();
        let service = HumanAnnotationService::new(
            storage.database().clone(),
            storage.content_root().to_path_buf(),
        );
        (dir, service)
    }

    #[tokio::test]
    async fn sealed_template_visual_uses_receipt_digest_when_content_is_bundled() {
        let (_dir, service) = service();
        let receipt_digest = format!("sha256:{}", "c".repeat(64));
        service.db.with_conn(|conn| {
            conn.execute(
                "UPDATE visual_revisions SET content_digest=NULL WHERE visual_id='vis_test' AND revision=1",
                [],
            )?;
            conn.execute(
                "INSERT INTO visual_seals(receipt_digest,visual_id,visual_revision,artifact_id,schema_version,compiler_name,compiler_version,runtime_digest,index_digest,data_digest,receipt_size_bytes,total_size_bytes,created_at) VALUES(?1,'vis_test',1,'visual:vis_test','synth.artifact-bundle.v1','workshop','test',?2,?2,?2,1,3,?3)",
                params![normalize_digest(&receipt_digest), "d".repeat(64), now()],
            )?;
            Ok(())
        }).unwrap();
        let mut sealed_task = task();
        sealed_task["subject"]["digest"] = Value::String(receipt_digest);
        let created = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(sealed_task),
                sealed_answer_keys: None,
                idempotency_key: "sealed-template".into(),
                reviewer_id: Some("reviewer".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        assert_eq!(created.state, "assigned");
    }

    #[tokio::test]
    async fn draft_is_private_and_submission_is_sealed_exportable_and_reopenable() {
        let (_dir, service) = service();
        let created = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(task()),
                sealed_answer_keys: None,
                idempotency_key: "test-key".into(),
                reviewer_id: Some("reviewer".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        let initial = service.open(created.session_id.clone()).await.unwrap();
        assert_eq!(
            initial.presentation.0["questions"]["preference"]["optionOrder"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let second = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(task()),
                sealed_answer_keys: None,
                idempotency_key: "test-key".into(),
                reviewer_id: Some("reviewer-02".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        assert_eq!(second.task_id, created.task_id);
        assert_ne!(second.session_id, created.session_id);
        service
            .answer(HumanAnnotationAnswerRequest {
                session_id: created.session_id.clone(),
                expected_revision: 0,
                question_id: "visible".into(),
                answer: crate::contract::specta::OpaqueJson(json!({"value":"yes"})),
            })
            .await
            .unwrap();
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: created.session_id.clone(),
                expected_revision: 1,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"headline"}),
                ),
                body_text: Some("The hierarchy is clear.".into()),
                audio_attachment_id: None,
            })
            .await
            .unwrap();
        let status = service.status(created.task_id.clone()).await.unwrap();
        assert_eq!(status.state, "in_progress");
        assert!(status.result_id.is_none());
        let submitted = service
            .submit(HumanAnnotationSubmitRequest {
                session_id: created.session_id.clone(),
                expected_revision: 2,
            })
            .await
            .unwrap();
        let status = service.status(created.task_id.clone()).await.unwrap();
        assert_eq!(
            status.result_id.as_deref(),
            Some(submitted.result_id.as_str())
        );
        let status_by_result = service.status(submitted.result_id.clone()).await.unwrap();
        assert_eq!(
            status_by_result.result_id.as_deref(),
            Some(submitted.result_id.as_str())
        );
        let sealed = service
            .sealed_result(submitted.result_id.clone())
            .await
            .unwrap();
        assert_eq!(sealed["state"], "submitted");
        assert_eq!(sealed["resultId"], submitted.result_id);
        assert_eq!(sealed["result"]["answers"]["visible"]["value"], "yes");
        assert_eq!(sealed["seal"]["resultId"], submitted.result_id);
        let export = service
            .export(HumanAnnotationExportRequest {
                result_id: submitted.result_id,
                format: "jsonl".into(),
            })
            .await
            .unwrap();
        assert!(service
            .store
            .exists("human_annotation_exports", &export.export_digest));
        let reopened = service.open(created.session_id).await.unwrap();
        assert_eq!(reopened.state, "submitted");
        assert!(reopened.result.is_some());
    }

    #[tokio::test]
    async fn audio_is_chunked_to_cas_before_it_can_be_commented_on() {
        let (_dir, service) = service();
        let created = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(task()),
                sealed_answer_keys: None,
                idempotency_key: "audio-key".into(),
                reviewer_id: None,
                created_by: None,
            })
            .await
            .unwrap();
        let begun = service
            .audio_begin(HumanAnnotationAudioBeginRequest {
                session_id: created.session_id.clone(),
                media_type: "audio/webm".into(),
                metadata: None,
            })
            .await
            .unwrap();
        service
            .audio_append(HumanAnnotationAudioChunkRequest {
                attachment_id: begun.attachment_id.clone(),
                chunk_index: 0,
                base64_data: base64::engine::general_purpose::STANDARD.encode(b"durable audio"),
            })
            .await
            .unwrap();
        let saved = service
            .audio_finish(HumanAnnotationAudioFinishRequest {
                attachment_id: begun.attachment_id.clone(),
                duration_ms: Some(1000),
            })
            .await
            .unwrap();
        assert_eq!(saved.state, "saved");
        let (bytes, mime) = service
            .audio_source(created.session_id.clone(), begun.attachment_id.clone())
            .await
            .unwrap();
        assert_eq!(bytes, b"durable audio");
        assert_eq!(mime, "audio/webm");
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: created.session_id,
                expected_revision: 0,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"audio-commentary"}),
                ),
                body_text: None,
                audio_attachment_id: Some(begun.attachment_id),
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn sealed_quiz_keys_never_enter_the_reviewer_task_and_grade_after_submit() {
        let (_dir, service) = service();
        let mut quiz = task();
        quiz["questions"] = json!([{"id":"fact","type":"quiz_single","prompt":"Which option best matches your reading of the evidence?","decisionCriteria":{"requiresHumanJudgment":true,"evidence":"Consider the retained fact in the evidence stage.","answerRule":"Choose the option that best matches your reading."},"required":true,"options":[{"id":"alpha"},{"id":"beta"}]}]);
        let created = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(quiz),
                sealed_answer_keys: Some(crate::contract::specta::OpaqueJson(
                    json!({"fact":"beta"}),
                )),
                idempotency_key: "quiz-key".into(),
                reviewer_id: Some("quiz-reviewer".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        let view = service.open(created.session_id.clone()).await.unwrap();
        assert!(!view.task.0.to_string().contains("sealed_answer_keys"));
        assert!(!view.task.0.to_string().contains("\"beta\":true"));
        service
            .answer(HumanAnnotationAnswerRequest {
                session_id: created.session_id.clone(),
                expected_revision: 0,
                question_id: "fact".into(),
                answer: crate::contract::specta::OpaqueJson(json!({"value":"beta"})),
            })
            .await
            .unwrap();
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: created.session_id.clone(),
                expected_revision: 1,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"subject"}),
                ),
                body_text: Some("The evidence supports beta.".into()),
                audio_attachment_id: None,
            })
            .await
            .unwrap();
        service
            .submit(HumanAnnotationSubmitRequest {
                session_id: created.session_id.clone(),
                expected_revision: 2,
            })
            .await
            .unwrap();
        let result = service
            .open(created.session_id)
            .await
            .unwrap()
            .result
            .unwrap()
            .0;
        assert_eq!(result["quizGrades"][0]["outcome"], "correct");
        assert!(result["quizGrades"][0].get("answerKeyDigest").is_some());
    }

    #[tokio::test]
    async fn preview_is_read_only_and_reports_provenance_only_evidence() {
        let (_dir, service) = service();
        let mut proposed = task();
        proposed["evidence"] = json!([{
            "kind":"file",
            "id":"artifact_file",
            "digest":format!("sha256:{}","c".repeat(64)),
            "path":"receipt.json"
        }]);
        let preview = service
            .preview(HumanAnnotationPreviewRequest {
                task: crate::contract::specta::OpaqueJson(proposed),
                sealed_answer_keys: None,
                reviewer_id: Some("preview-only".into()),
            })
            .await
            .unwrap();
        assert_eq!(preview.schema_version, PREVIEW_SCHEMA);
        assert_eq!(preview.evidence_count, 2);
        assert_eq!(preview.warnings.len(), 1);
        assert!(preview.warnings[0].contains("provenance only"));
        let task_count = service
            .db
            .with_conn(|conn| {
                conn.query_row("SELECT COUNT(*) FROM human_annotation_tasks", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(task_count, 0);
    }

    #[tokio::test]
    async fn campaign_adjudication_and_supersession_preserve_history() {
        let (_dir, service) = service();
        let first = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(task()),
                sealed_answer_keys: None,
                idempotency_key: "campaign-key".into(),
                reviewer_id: Some("reviewer-a".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        let second = service
            .create(HumanAnnotationCreateRequest {
                task: crate::contract::specta::OpaqueJson(task()),
                sealed_answer_keys: None,
                idempotency_key: "campaign-key".into(),
                reviewer_id: Some("reviewer-b".into()),
                created_by: Some("test".into()),
            })
            .await
            .unwrap();
        service
            .answer(HumanAnnotationAnswerRequest {
                session_id: first.session_id.clone(),
                expected_revision: 0,
                question_id: "visible".into(),
                answer: crate::contract::specta::OpaqueJson(json!({"value":"yes"})),
            })
            .await
            .unwrap();
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: first.session_id.clone(),
                expected_revision: 1,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"subject"}),
                ),
                body_text: Some("The evidence supports yes.".into()),
                audio_attachment_id: None,
            })
            .await
            .unwrap();
        let first_result = service
            .submit(HumanAnnotationSubmitRequest {
                session_id: first.session_id.clone(),
                expected_revision: 2,
            })
            .await
            .unwrap();
        service
            .answer(HumanAnnotationAnswerRequest {
                session_id: second.session_id.clone(),
                expected_revision: 0,
                question_id: "visible".into(),
                answer: crate::contract::specta::OpaqueJson(json!({"value":"no"})),
            })
            .await
            .unwrap();
        service
            .answer(HumanAnnotationAnswerRequest {
                session_id: second.session_id.clone(),
                expected_revision: 1,
                question_id: "hidden".into(),
                answer: crate::contract::specta::OpaqueJson(
                    json!({"value":"Evidence is incomplete."}),
                ),
            })
            .await
            .unwrap();
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: second.session_id.clone(),
                expected_revision: 2,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"subject"}),
                ),
                body_text: Some("The evidence supports no.".into()),
                audio_attachment_id: None,
            })
            .await
            .unwrap();
        let second_result = service
            .submit(HumanAnnotationSubmitRequest {
                session_id: second.session_id,
                expected_revision: 3,
            })
            .await
            .unwrap();

        let blocked_close = service
            .campaign_close(HumanAnnotationCampaignActionRequest {
                campaign_id: "campaign_test".into(),
                rationale: "Review complete.".into(),
            })
            .await
            .unwrap();
        assert_eq!(blocked_close["state"], "needs_adjudication");
        let adjudicated = service
            .campaign_adjudicate(HumanAnnotationCampaignAdjudicateRequest {
                campaign_id: "campaign_test".into(),
                result_ids: vec![
                    first_result.result_id.clone(),
                    second_result.result_id.clone(),
                ],
                decision: crate::contract::specta::OpaqueJson(
                    json!({"visible":"not_enough_evidence"}),
                ),
                rationale: "The evidence does not resolve the disagreement.".into(),
                adjudicator_id: Some("adjudicator".into()),
            })
            .await
            .unwrap();
        assert_eq!(adjudicated["adjudicationCount"], 1);
        let closed = service
            .campaign_close(HumanAnnotationCampaignActionRequest {
                campaign_id: "campaign_test".into(),
                rationale: "Disagreement is now explicitly adjudicated.".into(),
            })
            .await
            .unwrap();
        assert_eq!(closed["state"], "closed");

        let correction = service
            .supersede(HumanAnnotationSupersedeRequest {
                result_id: first_result.result_id.clone(),
                reason: "Correct the sealed judgment without overwriting it.".into(),
                reviewer_id: Some("reviewer-a-correction".into()),
            })
            .await
            .unwrap();
        service
            .comment(HumanAnnotationCommentRequest {
                session_id: correction.session_id.clone(),
                expected_revision: 0,
                evidence_digest: format!("sha256:{}", "a".repeat(64)),
                selector: crate::contract::specta::OpaqueJson(
                    json!({"kind":"visual_target","targetId":"correction"}),
                ),
                body_text: Some("Correction reviewed against the original evidence.".into()),
                audio_attachment_id: None,
            })
            .await
            .unwrap();
        let corrected = service
            .submit(HumanAnnotationSubmitRequest {
                session_id: correction.session_id,
                expected_revision: 1,
            })
            .await
            .unwrap();
        assert_ne!(corrected.result_id, first_result.result_id);
        let reopened = service.open(first.session_id).await.unwrap();
        assert!(reopened.result.is_some());
        let historical_export = service
            .export(HumanAnnotationExportRequest {
                result_id: first_result.result_id,
                format: "json".into(),
            })
            .await
            .unwrap();
        assert!(service
            .store
            .exists("human_annotation_exports", &historical_export.export_digest));
    }
}
