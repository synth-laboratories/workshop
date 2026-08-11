import openaiLogo from "../assets/openai-logo.svg";
import poolsideLogo from "../assets/poolside-logomark.svg";
import metaLogo from "../assets/meta-logomark.svg";
import { SynthLogo } from "./SynthLogo";

export type ProviderMarkKind = "openai" | "laguna" | "meta" | "synth";

/** Underlying model house — not the transport (OpenRouter vs direct).
 *  Synth-hosted Laguna stays Synth-branded; local / OpenRouter Laguna use Poolside. */
export function providerMarkForTarget(targetId: string): ProviderMarkKind {
	if (targetId === "openrouter-luna") return "openai";
	if (targetId === "local-laguna" || targetId === "openrouter-laguna-s") return "laguna";
	if (targetId === "openrouter-muse-spark") return "meta";
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
	if (kind === "meta") {
		return <img className={className} src={metaLogo} alt="" aria-hidden draggable={false} data-provider-mark="meta" />;
	}
	return <SynthLogo className={className} compact />;
}
