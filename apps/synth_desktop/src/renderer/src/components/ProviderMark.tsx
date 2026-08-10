import openaiLogo from "../assets/openai-logo.svg";
import poolsideLogo from "../assets/poolside-logomark.svg";
import { SynthLogo } from "./SynthLogo";

export type ProviderMarkKind = "openai" | "laguna" | "synth";

/** Underlying model house — not the transport (OpenRouter vs direct).
 *  Synth-hosted Laguna stays Synth-branded; local / OpenRouter Laguna use Poolside. */
export function providerMarkForTarget(targetId: string): ProviderMarkKind {
	if (targetId === "openrouter-luna") return "openai";
	if (targetId === "local-laguna" || targetId === "openrouter-laguna-s") return "laguna";
	return "synth";
}

export function ProviderMark({
	kind,
	className
}: {
	kind: ProviderMarkKind;
	className?: string;
}) {
	if (kind === "openai") {
		return (
			<img
				className={className}
				src={openaiLogo}
				alt=""
				aria-hidden
				draggable={false}
				data-provider-mark="openai"
			/>
		);
	}
	if (kind === "laguna") {
		return (
			<img
				className={className}
				src={poolsideLogo}
				alt=""
				aria-hidden
				draggable={false}
				data-provider-mark="poolside"
			/>
		);
	}
	return <SynthLogo className={className} compact />;
}
