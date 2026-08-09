import { useMemo, useState } from "react";
import { CONNECTOR_CATALOG } from "../runtime/connectorCatalog";

type Props = {
	onBack: () => void;
	onConfigure: (name: string) => void;
};

const CATEGORIES = ["Bundled", "Productivity", "Development"] as const;

export function ConnectorsPage({ onBack, onConfigure }: Props) {
	const [query, setQuery] = useState("");
	const normalized = query.trim().toLowerCase();
	const visible = useMemo(() => CONNECTOR_CATALOG.filter((connector) =>
		!normalized || `${connector.name} ${connector.description} ${connector.category}`.toLowerCase().includes(normalized)
	), [normalized]);

	return (
		<section className="connectors-page" data-testid="connectors-page">
			<header className="connectors-head">
				<div>
					<button type="button" className="page-back" onClick={onBack}>← Back</button>
					<h1>Connectors</h1>
					<p>MCP servers available to your agents.</p>
				</div>
				<button type="button" className="connectors-add" onClick={() => onConfigure("Custom MCP")}>+ Add Custom MCP</button>
			</header>
			<label className="connectors-search">
				<span aria-hidden>⌕</span>
				<input
					type="search"
					value={query}
					onChange={(event) => setQuery(event.target.value)}
					placeholder="Search connectors"
					aria-label="Search connectors"
				/>
			</label>
			<div className="connectors-scroll">
				{CATEGORIES.map((category) => {
					const connectors = visible.filter((connector) => connector.category === category);
					if (connectors.length === 0) return null;
					return (
						<section className="connector-group" key={category}>
							<h2>{category}</h2>
							<div className="connector-grid">
								{connectors.map((connector) => (
									<button
										type="button"
										className="connector-card"
										key={connector.id}
										onClick={() => onConfigure(connector.name)}
										aria-label={connector.bundled ? `${connector.name}, bundled` : `Configure ${connector.name}`}
									>
										<span className="connector-glyph" aria-hidden>{connector.glyph}</span>
										<span className="connector-copy">
											<strong>{connector.name}</strong>
											<small>{connector.description}</small>
										</span>
										<span className={`connector-status${connector.bundled ? " is-bundled" : ""}`}>
											{connector.bundled ? "Bundled" : "Not connected"}
										</span>
									</button>
								))}
							</div>
						</section>
					);
				})}
				{visible.length === 0 ? <p className="connectors-empty">No connectors match “{query}”.</p> : null}
			</div>
		</section>
	);
}

