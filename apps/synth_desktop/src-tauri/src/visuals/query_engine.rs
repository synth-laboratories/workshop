//! Revision-pinned, bounded analytical queries over host-supplied immutable rows.
//! Domain read models remain authoritative; this is an indexed derived corpus.
use crate::storage::Database;
use anyhow::{bail, Context, Result};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, OptionalExtension};
use serde_json::{json, Value};
use std::sync::Arc;

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("{key} required"))
}
fn path(field: &str) -> Result<String> {
    if field.len() > 200
        || field
            .split('.')
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        bail!("invalid query field");
    }
    Ok(format!("$.{field}"))
}
fn scalar(value: &Value) -> Result<SqlValue> {
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
        Value::Number(v) => SqlValue::Real(v.as_f64().context("finite number required")?),
        Value::String(v) => SqlValue::Text(v.clone()),
        _ => bail!("query operands must be scalar"),
    })
}
fn bind(value: SqlValue, params: &mut Vec<SqlValue>) -> String {
    params.push(value);
    format!("?{}", params.len())
}
fn predicate(expr: &Value, params: &mut Vec<SqlValue>, depth: usize) -> Result<String> {
    predicate_at(expr,params,depth,"row_json")
}
fn predicate_at(expr: &Value, params: &mut Vec<SqlValue>, depth: usize, root:&str) -> Result<String> {
    if depth > 16 {
        bail!("query exceeds maximum depth");
    }
    let op = text(expr, "op")?;
    if op == "all" {
        return Ok("1".into());
    }
    if op == "and" || op == "or" {
        let children = expr["expressions"]
            .as_array()
            .context("query expressions required")?;
        if children.len() > 64 {
            bail!("too many query predicates");
        }
        if children.is_empty() {
            return Ok(if op == "and" { "1" } else { "0" }.into());
        }
        let parts = children
            .iter()
            .map(|child| predicate_at(child, params, depth + 1, root))
            .collect::<Result<Vec<_>>>()?;
        return Ok(format!(
            "({})",
            parts.join(if op == "and" { " AND " } else { " OR " })
        ));
    }
    if op == "not" {
        return Ok(format!(
            "NOT ({})",
            predicate_at(&expr["expression"], params, depth + 1, root)?
        ));
    }
    let field = path(text(expr, "field")?)?;
    let p = bind(SqlValue::Text(field), params);
    let left = format!("json_extract({root},{p})");
    let kind = format!("json_type({root},{p})");
    if op=="any" || op=="sequence" {
        let a=format!("event_{depth}_a");
        let a_root=format!("CASE WHEN {a}.type='object' THEN {a}.value ELSE '{{}}' END");
        let before=predicate_at(if op=="any"{&expr["where"]}else{&expr["before"]},params,depth+1,&a_root)?;
        if op=="any" {
            return Ok(format!("COALESCE(({kind}='array' AND EXISTS(SELECT 1 FROM json_each({root},{p}) {a} WHERE {a}.type='object' AND ({before}))),0)"));
        }
        let b=format!("event_{depth}_b");
        let b_root=format!("CASE WHEN {b}.type='object' THEN {b}.value ELSE '{{}}' END");
        let after=predicate_at(&expr["after"],params,depth+1,&b_root)?;
        let sequence=bind(SqlValue::Text(path(text(expr,"sequenceField")?)?),params);
        let av=format!("json_extract({a_root},{sequence})");
        let bv=format!("json_extract({b_root},{sequence})");
        let gap=if let Some(gap)=expr.get("maxGap"){
            let n=gap.as_f64().filter(|n|n.is_finite()&&*n>=0.0).context("maxGap must be nonnegative")?;
            let bound=bind(SqlValue::Real(n),params);
            format!(" AND ({bv}-{av})<={bound}")
        }else{String::new()};
        return Ok(format!("COALESCE(({kind}='array' AND EXISTS(SELECT 1 FROM json_each({root},{p}) {a},json_each({root},{p}) {b} WHERE {a}.type='object' AND {b}.type='object' AND json_type({a_root},{sequence}) IN ('integer','real') AND json_type({b_root},{sequence}) IN ('integer','real') AND {bv}>{av}{gap} AND ({before}) AND ({after}))),0)"));
    }
    if op == "exists" {
        return Ok(format!(
            "{kind} IS {}NULL",
            if expr["exists"] == false { "" } else { "NOT " }
        ));
    }
    if op == "in" {
        let values = expr["value"].as_array().context("in requires an array")?;
        if values.len() > 100 {
            bail!("too many in operands");
        }
        let parts = values
            .iter()
            .map(|v| {
                predicate_at(
                    &json!({"op":"eq","field":expr["field"],"value":v}),
                    params,
                    depth + 1,
                    root,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(if parts.is_empty() {
            "0".into()
        } else {
            format!("({})", parts.join(" OR "))
        });
    }
    let value = expr.get("value").context("query value required")?;
    let rhs = bind(scalar(value)?, params);
    let type_test = match value {
        Value::Null => format!("{kind}='null'"),
        Value::Bool(v) => format!("{kind}='{}'", if *v { "true" } else { "false" }),
        Value::Number(_) => format!("{kind} IN ('integer','real')"),
        Value::String(_) => format!("{kind}='text'"),
        _ => bail!("scalar operand required"),
    };
    let equal = format!("COALESCE(({type_test} AND {left} IS {rhs}),0)");
    Ok(match op {
        "eq" => equal,
        "neq" => format!("NOT ({equal})"),
        "gt" | "gte" | "lt" | "lte" => {
            if !value.is_number() && !value.is_string() {
                bail!("ordered comparisons require a number or string");
            }
            let operator = match op {
                "gt" => ">",
                "gte" => ">=",
                "lt" => "<",
                _ => "<=",
            };
            format!("COALESCE(({type_test} AND {left} {operator} {rhs}),0)")
        }
        "contains" => {
            let element_type = match value {
                Value::Null => "j.type='null'",
                Value::Bool(true) => "j.type='true'",
                Value::Bool(false) => "j.type='false'",
                Value::Number(_) => "j.type IN ('integer','real')",
                _ => "j.type='text'",
            };
            let string = if value.is_string() {
                format!("({kind}='text' AND instr({left},{rhs})>0)")
            } else {
                "0".into()
            };
            format!("COALESCE((({kind}='array' AND EXISTS(SELECT 1 FROM json_each({root},{p}) j WHERE {element_type} AND j.value IS {rhs})) OR {string}),0)")
        }
        _ => bail!("unsupported query operator {op}"),
    })
}

pub async fn request(db: Arc<Database>, visual_id: String, request: Value) -> Result<Value> {
    let op = text(&request, "operation")?.to_owned();
    if op != "corpus.put" && request.to_string().len() > 2_097_152 {
        bail!("query request exceeds bounds");
    }
    if op=="corpus.from_collection" {
        return super::collection_corpus::materialize(db,visual_id,request).await;
    }
    let revision = request["revision"]
        .as_i64()
        .filter(|r| *r > 0)
        .context("positive visual revision required")?;
    let corpus = request.get("corpus").context("corpus required")?.clone();
    let id = text(&corpus, "id")?.to_owned();
    let cut = text(&corpus, "revision")?.to_owned();
    if op == "corpus.put" {
        let schema = text(&corpus, "schema")?.to_owned();
        let count = corpus["count"]
            .as_i64()
            .filter(|c| *c >= 0 && *c <= 1_000_000)
            .context("bounded corpus count required")?;
        let rows = request["rows"]
            .as_array()
            .filter(|rows| rows.len() <= 500)
            .context("at most 500 rows per batch")?
            .clone();
        let offset = request["offset"]
            .as_i64()
            .filter(|o| *o >= 0)
            .context("offset required")?;
        if offset + rows.len() as i64 > count || request.to_string().len() > 8_388_608 {
            bail!("corpus batch exceeds bounds");
        }
        return db.run_transaction(move|conn|{
            conn.execute("INSERT OR IGNORE INTO visual_corpora VALUES(?1,?2,?3,?4,?5,?6,0)",params![visual_id,revision,id,cut,schema,count])?;
            let (expected,sealed,known_schema):(i64,i64,String)=conn.query_row("SELECT expected_count,sealed,schema_id FROM visual_corpora WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4",params![visual_id,revision,id,cut],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            if count!=expected||schema!=known_schema{bail!("corpus revision metadata conflict");}
            for (index,row) in rows.into_iter().enumerate(){
                let row_id=text(&row,"id")?;let position=offset+index as i64;
                let existing:Option<(i64,String)>=conn.query_row("SELECT position,row_json FROM visual_corpus_rows WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4 AND row_id=?5",params![visual_id,revision,id,cut,row_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                if let Some((previous,raw))=existing{if previous!=position||serde_json::from_str::<Value>(&raw)?!=row{bail!("immutable corpus row conflict");}}
                else{if sealed!=0{bail!("corpus is sealed");}conn.execute("INSERT INTO visual_corpus_rows VALUES(?1,?2,?3,?4,?5,?6,?7)",params![visual_id,revision,id,cut,row_id,position,row.to_string()])?;}
            }
            let received:i64=conn.query_row("SELECT COUNT(*) FROM visual_corpus_rows WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4",params![visual_id,revision,id,cut],|r|r.get(0))?;
            if received==count{conn.execute("UPDATE visual_corpora SET sealed=1 WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4",params![visual_id,revision,id,cut])?;}
            Ok(json!({"received":received,"complete":received==count}))
        }).await;
    }
    db.run_read(move|conn|{
        let (count,sealed,schema):(i64,i64,String)=conn.query_row("SELECT expected_count,sealed,schema_id FROM visual_corpora WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4",params![visual_id,revision,id,cut],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if corpus["count"]!=count||corpus["schema"]!=schema{bail!("corpus identity does not match its registered source");}
        if sealed==0{bail!("corpus hydration incomplete");}
        if op=="corpus.detail" {
            let row_id=text(&request,"rowId")?;
            let raw:String=conn.query_row("SELECT details_json FROM visual_corpus_details WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4 AND row_id=?5",params![visual_id,revision,id,cut,row_id],|r|r.get(0)).optional()?.context("detail unavailable for this pinned source")?;
            if raw.len()>2_000_000 {bail!("detail exceeds 2 MB; use the domain paginated evidence reader");}
            return Ok(json!({"corpus":corpus,"rowId":row_id,"details":serde_json::from_str::<Value>(&raw)?}));
        }
        let spec=request.get("query").context("query required")?;
        if spec["schemaVersion"]!="synth.visuals-core.v1"{bail!("unsupported query schema");}
        let mut parameters=vec![SqlValue::Text(visual_id),SqlValue::Integer(revision),SqlValue::Text(id),SqlValue::Text(cut)];
        let predicate=predicate(&spec["where"],&mut parameters,0)?;
        let source=format!("FROM visual_corpus_rows WHERE visual_id=?1 AND visual_revision=?2 AND corpus_id=?3 AND corpus_revision=?4 AND ({predicate})");
        let total:i64=conn.query_row(&format!("SELECT COUNT(*) {source}"),params_from_iter(parameters.iter()),|r|r.get(0))?;
        if op=="corpus.aggregate"{
            let field=text(&request,"field")?;let path=path(field)?;
            let p=bind(SqlValue::Text(path),&mut parameters);
            let objects:i64=conn.query_row(&format!("SELECT COUNT(*) {source} AND json_type(row_json,{p})='object'"),params_from_iter(parameters.iter()),|r|r.get(0))?;
            if objects>0{bail!("aggregate fields must contain scalars or scalar arrays");}
            let sql=format!("WITH selected AS (SELECT row_json {source}), members AS (SELECT DISTINCT json_extract(row_json,'$.id') AS row_id,CASE WHEN j.type IN ('integer','real') THEN 'number' ELSE j.type END AS value_type,j.value AS value FROM selected,json_each(row_json,{p}) j UNION ALL SELECT json_extract(row_json,'$.id'),'missing',NULL FROM selected WHERE json_type(row_json,{p}) IS NULL) SELECT value_type,value,COUNT(*) FROM members GROUP BY value_type,value ORDER BY COUNT(*) DESC,value LIMIT 1001");
            let mut statement=conn.prepare(&sql)?;
            let rows=statement.query_map(params_from_iter(parameters.iter()),|r|Ok((r.get::<_,String>(0)?,r.get::<_,SqlValue>(1)?,r.get::<_,i64>(2)?)))?;
            let mut buckets=Vec::new();let mut missing=0;
            for row in rows{
                let(kind,value,n)=row?;
                if kind=="object"||kind=="array"{bail!("aggregate fields must contain scalars or scalar arrays");}
                if buckets.len()>=1000{bail!("aggregate exceeds 1000 buckets; narrow the cohort");}
                let value=match kind.as_str(){"null"=>Value::Null,"true"=>json!(true),"false"=>json!(false),_=>match value{SqlValue::Integer(v)=>json!(v),SqlValue::Real(v)=>json!(v),SqlValue::Text(v)=>json!(v),_=>Value::Null}};
                let key=if kind=="missing"{"(missing)".into()}else if value.is_null(){"(null)".into()}else{value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string())};
                let query=if kind=="missing"{missing=n;json!({"op":"exists","field":field,"exists":false})}else{json!({"op":"or","expressions":[{"op":"eq","field":field,"value":value},{"op":"contains","field":field,"value":value}]})};
                let mut bucket=json!({"key":key,"value":value,"count":n,"denominator":total,"ratio":if total>0{n as f64/total as f64}else{0.0},"query":{"schemaVersion":"synth.visuals-core.v1","where":{"op":"and","expressions":[spec["where"],query]}}});
                if kind=="missing"{bucket.as_object_mut().unwrap().remove("value");}buckets.push(bucket);
            }
            // Match the portable scalar ordering, not SQLite's mixed-type order
            // or a platform locale: missing, null, boolean, number, string.
            buckets.sort_by(|a,b| {
                let rank=|v:Option<&Value>|match v {None=>0,Some(Value::Null)=>1,Some(Value::Bool(_))=>2,Some(Value::Number(_))=>3,_=>4};
                b["count"].as_i64().cmp(&a["count"].as_i64()).then_with(|| {
                    let av=a.get("value");let bv=b.get("value");
                    rank(av).cmp(&rank(bv)).then_with(||match (av,bv) {
                        (Some(Value::Bool(a)),Some(Value::Bool(b)))=>a.cmp(b),
                        (Some(Value::Number(a)),Some(Value::Number(b)))=>a.as_f64().unwrap().total_cmp(&b.as_f64().unwrap()),
                        (Some(Value::String(a)),Some(Value::String(b)))=>a.cmp(b),
                        _=>std::cmp::Ordering::Equal,
                    })
                })
            });
            return Ok(json!({"corpus":corpus,"sourceCohortId":request["cohortId"],"field":field,"buckets":buckets,"denominator":total,"missing":missing,"exactness":"exact","completeness":"complete"}));
        }
        if op=="corpus.sample"{
            conn.create_scalar_function("visual_sample_rank",2,rusqlite::functions::FunctionFlags::SQLITE_UTF8|rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,|ctx|{
                let seed:i64=ctx.get(0)?;let id:String=ctx.get(1)?;let text=json!({"id":id,"seed":seed}).to_string();let mut hash=0xcbf29ce484222325u64;
                for byte in text.bytes(){hash=(hash^u64::from(byte)).wrapping_mul(0x100000001b3);}
                Ok(format!("fnv1a64:{hash:016x}"))
            })?;
            let strategy=text(&request,"strategy")?;let requested=request["count"].as_i64().unwrap_or(8).clamp(0,100);
            let seed=request["options"]["seed"].as_i64().unwrap_or(1);
            let score=path(request["options"]["scoreField"].as_str().unwrap_or("reward"))?;
            let score=bind(SqlValue::Text(score),&mut parameters);let numeric=format!("json_extract(row_json,{score})");
            let scored_source=format!("{source} AND json_type(row_json,{score}) IN ('integer','real')");
            let rank=match strategy{
                "random"|"diverse"=>{let seed=bind(SqlValue::Integer(seed),&mut parameters);format!("visual_sample_rank({seed},row_id)")},
                "boundary"=>format!("abs({numeric})"),
                "outlier"=>format!("abs({numeric}-(SELECT AVG({numeric}) {scored_source})) DESC"),
                "representative"=>format!("abs({numeric}-(SELECT {numeric} {scored_source} ORDER BY {numeric} LIMIT 1 OFFSET (SELECT COUNT(*)/2 {scored_source})))"),
                "failure"=>"position".into(),
                _=>bail!("unsupported sampling strategy")
            };
            let extra=if strategy=="failure"{let p=bind(SqlValue::Text(path(request["options"]["failureField"].as_str().unwrap_or("failed"))?),&mut parameters);format!(" AND json_type(row_json,{p})='true'")}else if ["boundary","outlier","representative"].contains(&strategy){format!(" AND json_type(row_json,{score}) IN ('integer','real')")}else{String::new()};
            let sql=if strategy=="diverse"{format!("WITH ranked AS (SELECT row_json,ROW_NUMBER() OVER(ORDER BY {rank},row_id)-1 AS n {source}{extra}) SELECT row_json FROM ranked WHERE n % {}=0 ORDER BY n LIMIT {requested}",std::cmp::max(1,total/std::cmp::max(1,requested)))}else{format!("SELECT row_json {source}{extra} ORDER BY {rank},row_id LIMIT {requested}")};
            let mut statement=conn.prepare(&sql)?;let rows=statement.query_map(params_from_iter(parameters.iter()),|r|r.get::<_,String>(0))?.map(|row|Ok(serde_json::from_str::<Value>(&row?)?)).collect::<Result<Vec<_>>>()?;
            return Ok(json!({"rows":rows,"strategy":strategy,"seed":seed,"requested":requested,"sourceCount":total,"exactness":"exact"}));
        }
        if op!="corpus.query"{bail!("unsupported corpus operation");}
        let limit=request["window"]["limit"].as_i64().unwrap_or(100);let offset=request["window"]["offset"].as_i64().unwrap_or(0);
        if !(0..=1000).contains(&limit)||offset<0{bail!("invalid query window");}
        let mut order=Vec::new();
        if let Some(orders)=spec["orderBy"].as_array(){if orders.len()>8{bail!("too many sort keys");}for entry in orders{
            let p=bind(SqlValue::Text(path(text(entry,"field")?)?),&mut parameters);
            let direction=match text(entry,"direction")?{"asc"=>"ASC","desc"=>"DESC",_=>bail!("invalid sort direction")};
            order.push(format!("CASE json_type(row_json,{p}) WHEN 'null' THEN 1 WHEN 'false' THEN 2 WHEN 'true' THEN 2 WHEN 'integer' THEN 3 WHEN 'real' THEN 3 WHEN 'text' THEN 4 WHEN 'object' THEN 5 WHEN 'array' THEN 5 ELSE 0 END {direction},json_extract(row_json,{p}) {direction}"));
        }}
        order.push(if order.is_empty(){"position ASC"}else{"row_id ASC"}.into());
        let l=bind(SqlValue::Integer(limit),&mut parameters);let o=bind(SqlValue::Integer(offset),&mut parameters);
        let mut statement=conn.prepare(&format!("SELECT row_json {source} ORDER BY {} LIMIT {l} OFFSET {o}",order.join(",")))?;
        let rows=statement.query_map(params_from_iter(parameters.iter()),|r|r.get::<_,String>(0))?.map(|row|Ok(serde_json::from_str::<Value>(&row?)?)).collect::<Result<Vec<_>>>()?;
        Ok(json!({"corpus":corpus,"query":spec,"total":total,"rows":rows,"window":{"offset":offset,"limit":limit},"excluded":count-total,"exactness":"exact","completeness":"complete"}))
    }).await
}
