import { sveltekit } from '@sveltejs/kit/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
    plugins: [sveltekit()],
    test: {
        pool: 'forks',
        testTimeout: 30_000,
        hookTimeout: 120_000,
        include: ['src/**/*.test.ts'],
    },
})
