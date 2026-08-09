"use client";

import {
	MoonStar
} from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { Tabs } from "@/components/ui/tabs";

import AsyncDelegationDesk from "./AsyncDelegationDesk";
import EffortBoard from "./EffortBoard";
import SyncCockpit from "./SyncCockpit";


type InternShell = "sync" | "async";

/** Effort program board is primary; the runtime desk is ops/debug. */
export type InternAsyncView = "efforts" | "runtime";

/** Chooses different runtime products, never a cosmetic mode on one session. */
export default function InternRuntimeClient({
	initialShell = "sync",
	initialAsyncView = "efforts"
}: {
	initialShell?: InternShell;
	initialAsyncView?: InternAsyncView;
}) {
	const [shell, setShell] = useState<InternShell>(initialShell);
	const [asyncView, setAsyncView] = useState<InternAsyncView>(initialAsyncView);
	const router = useRouter();
	const selectShell = (nextShell: InternShell) => {
		setShell(nextShell);
		router.replace(`/smr/intern/${nextShell}`);
	};

	return (
		<div className="min-h-full overflow-x-hidden">
			<div data-testid="intern-runtime-shell" className="sticky top-0 z-30 border-b border-border/70 bg-background/95 px-3 py-2 backdrop-blur md:px-5">
				<div className="mx-auto flex max-w-[1900px] items-center justify-between gap-3">
					<div className="min-w-0">
						<p className="truncate text-sm font-semibold">Research Intern</p>
						<p className="hidden text-xs text-muted-foreground sm:block">Live research workspace</p>
					</div>
					<Tabs
						value={shell}
						onChange={selectShell}
						variant="pill"
						items={[
							{ id: "sync", label: "Sync" },
							{ id: "async", label: "Async" }
						]}
					/>
				</div>
			</div>

			{shell === "sync"
				? <SyncCockpit />
				: <div>
					<span className="sr-only">
						<MoonStar />
						Autonomous runtime
					</span>

					<div data-testid="intern-async-view-switch" className="border-b border-border/60 px-3 py-2 md:px-5">
						<div className="mx-auto flex max-w-[1900px] items-center gap-3">
							<Tabs
								value={asyncView}
								onChange={setAsyncView}
								variant="pill"
								size="sm"
								items={[
									{ id: "efforts", label: "Efforts" },
									{ id: "runtime", label: "Runtime desk" }
								]}
							/>
							<p className="hidden text-xs text-muted-foreground sm:block">
								Efforts organize Async work; the runtime desk is ops and debug.
							</p>
						</div>
					</div>

					{asyncView === "efforts"
						? <EffortBoard onOpenRuntimeDesk={() => setAsyncView("runtime")} />
						: <AsyncDelegationDesk onOpenEffortBoard={() => setAsyncView("efforts")} />}
				</div>}
		</div>
	);
}
