/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const fpc_new_game: () => [number, number];
export const fpc_legal_moves: (a: number, b: number) => [number, number];
export const fpc_best_move: (a: number, b: number, c: number, d: number) => [number, number];
export const fpc_eval: (a: number, b: number) => [number, number];
export const fpc_analyze: (a: number, b: number, c: number) => [number, number];
export const fpc_attack_map: (a: number, b: number, c: number, d: number) => [number, number];
export const __wbindgen_externrefs: WebAssembly.Table;
export const __wbindgen_malloc: (a: number, b: number) => number;
export const __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
export const __wbindgen_free: (a: number, b: number, c: number) => void;
export const __wbindgen_start: () => void;
