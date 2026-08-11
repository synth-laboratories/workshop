import type {
	ActivityEvent,
	ArtifactRef,
	AsyncInternPin,
	ChatMessage,
	LandingScenarioId,
	LandingState,
	LocalChat,
	SyncSession
} from "../types/landing";

const ARTIFACT_CRAFTAX_PARETO: ArtifactRef = {
	id: "art-craftax-pareto",
	kind: "score_series",
	title: "Craftax cost vs performance",
	summary: "Achievements against $/rollout — same surface as usesynth.ai/evals/craftax, native in Desktop.",
	messageId: "lm4",
	shownByAgent: true,
	preview: {
		variant: "craftax_pareto",
		metrics: [
			{ label: "Laguna XS ach.", value: "11.4" },
			{ label: "$ / rollout", value: "$0.12" },
			{ label: "vs Flash", value: "+2.1 ach" }
		]
	}
};

const ARTIFACT_CRAFTAX_FRAME: ArtifactRef = {
	id: "art-craftax-frame",
	kind: "environment_frame",
	title: "Rollout 482 · step 37",
	summary: "Failure cluster after first wood — canvas + accessible state.",
	messageId: "m3",
	shownByAgent: true,
	preview: {
		variant: "craftax_frame",
		metrics: [{ label: "State", value: "Player (12,17) HP7 wood3" }]
	}
};

/** Local Laguna chat — Poolside-style bubbles + expandable activity lines. */
const CHAT_PORTING: LocalChat = {
	id: "chat-1",
	title: "do you see our work on porting…",
	messages: [
		{
			id: "lm1",
			role: "user",
			body: "hello?",
			at: "14:58:01"
		},
		{
			id: "lm2",
			role: "assistant",
			body: "Hello! I'm here to help. How can I assist you today?",
			at: "14:58:03"
		},
		{
			id: "lm3",
			role: "user",
			body: "do you see our work on porting emerald to rust?",
			at: "14:58:40"
		},
		{
			id: "lm4",
			role: "assistant",
			body: "Yes — and here’s a Craftax cost/perf visual you can open beside this chat (same surface as the public evals page).",
			at: "14:59:12"
		}
	],
	activityByMessageId: {
		lm2: [{ id: "la1", label: "… Thought", kind: "thought" }],
		lm4: [
			{ id: "la2", label: "… Thought", kind: "thought" },
			{
				id: "la3",
				label: "… Searched once, ran 2 commands",
				kind: "search",
				detail:
					"$ rg -n \"emerald\" crates/ src/\ncrates/emerald-sys/src/lib.rs:12: //! FFI bindings for emerald\ncrates/emerald-core/src/port.rs:4: // TODO: finish C++ shim removal\n\n$ find . -name '*emerald*' -type f | head -20\n./crates/emerald-sys/Cargo.toml\n./crates/emerald-core/src/port.rs\n./docs/porting-emerald.md"
			},
			{
				id: "la3b",
				label: "Read",
				kind: "file_read",
				path: "~/Documents/GitHub/gamebench/tasks/pokemon-emerald-littleroot-singleplayer/PROGRESS.md"
			},
			{
				id: "la3c",
				label: "Read",
				kind: "file_read",
				path: "~/Documents/GitHub/gamebench/tasks/pokemon-emerald-littleroot-singleplayer/gold_rust/src/lib.rs"
			},
			{ id: "la4", label: "… Thought", kind: "thought" },
			{
				id: "la5",
				label: "… Created visual · Craftax cost vs performance",
				kind: "visual",
				detail: "artifact_kind=score_series · shown_by_agent=true · click to open"
			}
		]
	},
	artifacts: [ARTIFACT_CRAFTAX_PARETO]
};

const CHAT_HARNESS: LocalChat = {
	id: "chat-2",
	title: "compare harness r92 vs r93",
	messages: [
		{
			id: "hm1",
			role: "user",
			body: "compare harness r92 vs r93 — anything scary?",
			at: "11:02:00"
		},
		{
			id: "hm2",
			role: "assistant",
			body: "r93 tightens seed isolation. One flaky timeout got worse; otherwise diffs look intentional.",
			at: "11:02:40"
		}
	],
	activityByMessageId: {
		hm2: [
			{
				id: "ha1",
				label: "… Thought"
			},
			{
				id: "ha2",
				label: "… Ran 1 command",
				detail: "$ git diff harness/r92 harness/r93 --stat\n 12 files changed, 184 insertions(+), 61 deletions(-)"
			}
		]
	}
};

const CHAT_EVAL: LocalChat = {
	id: "chat-3",
	title: "summarize last night’s eval diffs",
	messages: [
		{
			id: "em1",
			role: "user",
			body: "summarize last night’s eval diffs",
			at: "09:12:00"
		},
		{
			id: "em2",
			role: "assistant",
			body: "Three suites moved: craftax +1.2pp, webshop flat, algebra −0.4pp on the long tail.",
			at: "09:12:22"
		}
	]
};

/** Sync live session — product mailbox + evidence Codex activity interleaved. */
const SYNC_CRAFTAX: SyncSession = {
	id: "sync-1",
	title: "Live · craftax failure triage",
	status: "thinking",
	remoteId: "smr.intern-sync-session.v1/sync_sess_craftax_01",
	cursor: 11,
	messages: [
		{
			id: "m1",
			role: "user",
			body: "Triage the latest Craftax rollout failures and tell me if this is harness noise or a real regression.",
			at: "15:02:11"
		},
		{
			id: "m2",
			role: "assistant",
			body: "Pulling the bound run transcript and comparing failure clusters against harness r92. I’ll keep the Sync session live while I dig.",
			at: "15:02:18"
		},
		{
			id: "m3",
			role: "assistant",
			body: "73% of the cluster fires right after first wood acquisition — looks real, not noise. Opening the step-37 frame + achievement strip as a Desktop visual.",
			at: "15:04:02"
		}
	],
	artifacts: [ARTIFACT_CRAFTAX_FRAME],
	activity: [
		{
			sequence: 1,
			eventKind: "session.created",
			lane: "intern",
			summary: "Sync session created · generation 0",
			at: "15:02:10",
			detail: "runtime=sync · commander queue opened for operator"
		},
		{
			sequence: 2,
			eventKind: "command.receipt",
			lane: "intern",
			summary: "operator_message · applied",
			at: "15:02:11",
			detail: "command_kind=operator_message · expected_generation=0 · actor=user"
		},
		{
			sequence: 3,
			eventKind: "turn/started",
			lane: "codex",
			summary: "Codex turn started",
			at: "15:02:12",
			detail:
				"source=/smr/internal/codex-activity/stream\nevidence only — not product authority"
		},
		{
			sequence: 4,
			eventKind: "reasoning",
			lane: "codex",
			summary: "… Thinking · harness compare plan",
			at: "15:02:14",
			detail:
				"Normalize failure clusters against harness r92.\nPrefer first-wood-acquisition cut; check timeout noise second."
		},
		{
			sequence: 5,
			eventKind: "mcp_tool_call",
			lane: "codex",
			summary: "mcp · smr_list_runs",
			at: "15:02:16",
			detail:
				"tool=smr_list_runs\nargs={ project: \"craftax-research\", limit: 20 }\nstatus=ok · 6 runs"
		},
		{
			sequence: 6,
			eventKind: "mcp_tool_call",
			lane: "codex",
			summary: "mcp · smr_get_swarm_activity",
			at: "15:02:28",
			detail:
				"tool=smr_get_swarm_activity\nargs={ run_id: \"run_craftax_482\" }\nstatus=ok · 128 episodes"
		},
		{
			sequence: 7,
			eventKind: "command_execution",
			lane: "codex",
			summary: "shell · rg wood-acquisition cluster",
			at: "15:02:40",
			detail:
				"$ rg -n \"wood acquisition|first_wood\" rollout_logs/ | head -40\nexit=0 · 31 hits near t≈wood_1"
		},
		{
			sequence: 8,
			eventKind: "reasoning",
			lane: "codex",
			summary: "… Thought · cluster looks real",
			at: "15:03:50",
			detail:
				"73% of failures share the first-wood cut.\nHarness r92 does not inject that failure mode → regression."
		},
		{
			sequence: 9,
			eventKind: "agent_message",
			lane: "codex",
			summary: "Draft reply ready (evidence)",
			at: "15:03:58",
			detail: "Not mailbox authority — projected into operator message next"
		},
		{
			sequence: 10,
			eventKind: "resource_presentation_requested",
			lane: "intern",
			summary: "Visual · reward-by-revision draft",
			at: "15:04:01",
			detail: "resource_refs → SMR visual · commander queue product event"
		},
		{
			sequence: 11,
			eventKind: "command.receipt",
			lane: "intern",
			summary: "heartbeat · after_sequence=14",
			at: "15:04:05",
			detail: "command_kind=heartbeat · actor=instance"
		}
	]
};

const ASYNC_SLEEPING: AsyncInternPin = {
	phase: "sleeping",
	summary: "Idle · last checkpoint 2h ago",
	cycle: 17,
	checkpointId: "ckpt_async_17",
	remoteId: "smr.intern-async-runtime.v1/org",
	cursor: 42,
	messages: [
		{
			id: "a1",
			role: "user",
			body: "Keep watching the craftax factory overnight. Checkpoint when you have a regression story.",
			at: "13:10:00"
		},
		{
			id: "a2",
			role: "assistant",
			body: "Cycle 17 complete. Published checkpoint with affected trajectories. Sleeping until next wake — closing the desktop will not pause me.",
			at: "13:18:22"
		}
	],
	activity: [
		{
			sequence: 36,
			eventKind: "cycle_started",
			lane: "intern",
			summary: "Async cycle 17 started",
			at: "13:12:01"
		},
		{
			sequence: 37,
			eventKind: "command.receipt",
			lane: "intern",
			summary: "message · applied · generation 11",
			at: "13:12:02"
		},
		{
			sequence: 38,
			eventKind: "mcp_tool_call",
			lane: "codex",
			summary: "smr_get_swarm_status · factory craftax",
			at: "13:12:08",
			detail: "Codex activity evidence stream"
		},
		{
			sequence: 39,
			eventKind: "file_change",
			lane: "codex",
			summary: "Wrote checkpoint notes in workspace",
			at: "13:17:44"
		},
		{
			sequence: 40,
			eventKind: "checkpoint_published",
			lane: "intern",
			summary: "async_checkpoint_scheduled · ckpt_async_17",
			at: "13:18:10",
			detail: "next_wake_at set · leave_safe=true"
		},
		{
			sequence: 41,
			eventKind: "metric.recorded",
			lane: "intern",
			summary: "128 episodes · 34 failures · 7.1 min",
			at: "13:18:12"
		},
		{
			sequence: 42,
			eventKind: "runtime.sleeping",
			lane: "intern",
			summary: "SLEEPING until wake — disconnect ≠ pause",
			at: "13:18:22"
		}
	]
};

const ASYNC_RUNNING: AsyncInternPin = {
	phase: "running",
	summary: "Watching craftax factory · cycle 18",
	cycle: 18,
	checkpointId: "ckpt_async_17",
	remoteId: "smr.intern-async-runtime.v1/org",
	cursor: 48,
	messages: [
		...ASYNC_SLEEPING.messages,
		{
			id: "a3",
			role: "system",
			body: "Woke for cycle 18 · continuing factory watch",
			at: "15:05:00"
		}
	],
	activity: [
		...ASYNC_SLEEPING.activity,
		{
			sequence: 43,
			eventKind: "cycle_started",
			lane: "intern",
			summary: "Async cycle 18 started",
			at: "15:05:00"
		},
		{
			sequence: 44,
			eventKind: "turn/started",
			lane: "codex",
			summary: "Codex turn (evidence)",
			at: "15:05:01"
		},
		{
			sequence: 45,
			eventKind: "command_execution",
			lane: "codex",
			summary: "shell · sample latest rollout shard",
			at: "15:05:12"
		}
	]
};

const ASYNC_NEEDS_INPUT: AsyncInternPin = {
	phase: "waiting_for_input",
	summary: "Needs approval on spend bump",
	needsInput: true,
	cycle: 19,
	checkpointId: "ckpt_async_18",
	remoteId: "smr.intern-async-runtime.v1/org",
	cursor: 55,
	messages: [
		{
			id: "n1",
			role: "assistant",
			body: "I’m parked on a spend bump for the craftax factory (+$40). Approve or redirect before I continue cycle 19.",
			at: "15:08:40"
		}
	],
	activity: [
		{
			sequence: 52,
			eventKind: "parked_question",
			lane: "intern",
			summary: "HUMAN_ANSWER · spend bump approval",
			at: "15:08:40",
			detail: "provide_input / answer_interaction"
		},
		{
			sequence: 53,
			eventKind: "runtime.blocked",
			lane: "intern",
			summary: "waiting_for_input · leave_safe still true",
			at: "15:08:41"
		}
	]
};

function emptyMsgs(): ChatMessage[] {
	return [];
}
function emptyAct(): ActivityEvent[] {
	return [];
}

export const LANDING_SCENARIOS: Record<LandingScenarioId, LandingState> = {
	"landing-first-run": {
		id: "landing-first-run",
		label: "First run",
		chats: [],
		syncSessions: [],
		asyncIntern: null,
		model: { status: "not_installed", name: "Laguna XS 2.1" },
		selectedTargetId: "local-laguna",
		composerEnabled: false
	},
	"landing-downloading": {
		id: "landing-downloading",
		label: "Downloading model",
		chats: [CHAT_PORTING],
		syncSessions: [SYNC_CRAFTAX],
		asyncIntern: ASYNC_SLEEPING,
		model: {
			status: "downloading",
			name: "Laguna XS 2.1 NVFP4",
			downloadProgress: 42,
			downloadPaused: false
		},
		selectedTargetId: "local-laguna",
		composerEnabled: false
	},
	"landing-ready": {
		id: "landing-ready",
		label: "Model ready",
		chats: [],
		syncSessions: [],
		asyncIntern: ASYNC_RUNNING,
		model: { status: "ready", name: "Laguna XS 2.1" },
		selectedTargetId: "local-laguna",
		composerEnabled: true
	},
	"landing-with-history": {
		id: "landing-with-history",
		label: "With chat history",
		chats: [CHAT_PORTING, CHAT_HARNESS, CHAT_EVAL],
		syncSessions: [
			SYNC_CRAFTAX,
			{
				id: "sync-2",
				title: "Live · harness bundle review",
				status: "ready",
				remoteId: "smr.intern-sync-session.v1/sync_sess_harness_02",
				cursor: 6,
				messages: [
					{
						id: "h1",
						role: "user",
						body: "Review the harness bundle diff before we promote r93.",
						at: "14:40:00"
					},
					{
						id: "h2",
						role: "assistant",
						body: "Bundle looks clean aside from one flaky seed. Ready when you are.",
						at: "14:41:12"
					}
				],
				activity: [
					{
						sequence: 1,
						eventKind: "session.created",
						lane: "intern",
						summary: "Sync session created",
						at: "14:40:00"
					},
					{
						sequence: 2,
						eventKind: "command.receipt",
						lane: "intern",
						summary: "operator_message · applied",
						at: "14:40:01"
					},
					{
						sequence: 3,
						eventKind: "mcp_tool_call",
						lane: "codex",
						summary: "smr_get_harness_bundle",
						at: "14:40:20"
					}
				]
			}
		],
		asyncIntern: ASYNC_NEEDS_INPUT,
		model: { status: "ready", name: "Laguna XS 2.1" },
		selectedTargetId: "local-laguna",
		composerEnabled: true
	},
	"landing-with-project": {
		id: "landing-with-project",
		label: "With project + cloud",
		chats: [
			{
				id: "chat-1",
				title: "inspect rollout 482 failures",
				messages: [
					{
						id: "pm1",
						role: "user",
						body: "inspect rollout 482 failures",
						at: "10:00:00"
					},
					{
						id: "pm2",
						role: "assistant",
						body: "482 is clustered on timeout after first resource gather. Local Laguna can dig the shard; Cloud Sync if you want Intern on the factory.",
						at: "10:00:30"
					}
				],
				activityByMessageId: {
					pm2: [{ id: "pa1", label: "… Thought" }]
				}
			},
			{
				id: "chat-2",
				title: "local Laguna tok/s check",
				messages: [
					{
						id: "tm1",
						role: "user",
						body: "what tok/s are we getting on Metal?",
						at: "10:20:00"
					},
					{
						id: "tm2",
						role: "assistant",
						body: "About 38 tok/s decode on this Mac with NVFP4 Laguna XS. Prefill is the bottleneck on long contexts.",
						at: "10:20:12"
					}
				]
			}
		],
		syncSessions: [
			{
				id: "sync-1",
				title: "Live · wire Intern into craftax",
				status: "waiting_for_operator",
				remoteId: "smr.intern-sync-session.v1/sync_sess_wire_03",
				cursor: 9,
				messages: [
					{
						id: "w1",
						role: "assistant",
						body: "I need an approval to attach the factory binding before I continue.",
						at: "15:01:00"
					}
				],
				activity: [
					{
						sequence: 8,
						eventKind: "sync_waiting_for_operator",
						lane: "intern",
						summary: "Presence lease · answer_interaction required",
						at: "15:01:00"
					},
					{
						sequence: 9,
						eventKind: "item/completed",
						lane: "codex",
						summary: "Draft attach plan (evidence)",
						at: "15:00:58"
					}
				]
			},
			{
				id: "sync-2",
				title: "Live · factory health pass",
				status: "paused",
				remoteId: "smr.intern-sync-session.v1/sync_sess_health_04",
				cursor: 4,
				messages: emptyMsgs(),
				activity: emptyAct()
			}
		],
		asyncIntern: ASYNC_RUNNING,
		model: { status: "ready", name: "Laguna XS 2.1" },
		selectedTargetId: "intern-sync",
		composerEnabled: true
	}
};

export const SCENARIO_ORDER = Object.keys(LANDING_SCENARIOS) as LandingScenarioId[];
