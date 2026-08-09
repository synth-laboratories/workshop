import logoHero from "../assets/synth-logo-hero.png";
import logoTab from "../assets/synth-logo-tab.png";

/**
 * Official Synth mark — branching MCMC / circuit glow
 * from frontend `public/images/synth-logo.png` + `SynthIcon`.
 */
export function SynthLogo({
	className,
	compact = false
}: {
	className?: string;
	compact?: boolean;
}) {
	return (
		<img
			className={className}
			src={compact ? logoTab : logoHero}
			alt={compact ? "" : "Synth"}
			aria-hidden={compact || undefined}
			draggable={false}
		/>
	);
}
