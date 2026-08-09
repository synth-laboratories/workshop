/// <reference types="vite/client" />

export {};

declare global {
	interface Window {
		synthDesktop?: {
			platform: string;
		};
	}
}
