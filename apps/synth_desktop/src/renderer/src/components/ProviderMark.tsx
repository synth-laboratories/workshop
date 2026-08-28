import openaiLogo from "../assets/openai-logo.svg";
import poolsideLogo from "../assets/poolside-logomark.svg";
import metaLogo from "../assets/meta-logomark.svg";
import googleLogo from "../assets/google-logomark.svg";
import { isOpenRouterTargetId } from "../types/landing";
import { SynthLogo } from "./SynthLogo";

export type ProviderMarkKind = "openai" | "laguna" | "meta" | "google" | "openrouter" | "synth";

/** Source-owned providers retain their own marks. All OpenRouter catalog
 * entries use the OpenRouter mark so a custom slug is never misidentified. */
export function providerMarkForTarget(targetId: string): ProviderMarkKind {
	if (targetId === "local-laguna") return "laguna";
	if (targetId.startsWith("synth-cloud-laguna")) return "laguna";
	if (isOpenRouterTargetId(targetId)) return "openrouter";
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
	if (kind === "google") {
		return <img className={className} src={googleLogo} alt="" aria-hidden draggable={false} data-provider-mark="google" />;
	}
	if (kind === "openrouter") {
		return <span className={className} aria-hidden data-provider-mark="openrouter">OR</span>;
	}
	return <SynthLogo className={className} compact />;
}
