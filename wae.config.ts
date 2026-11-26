/**
 * Build and release configuration for OpenCAM.
 *
 * Pure literals only: `wae` parses this file statically and never executes it.
 * Build order is fixed: native → ts → staging.
 *
 * The package lives under `projects/packages`; the native addon is built from
 * the `open-cam-napi` crate and staged into the per-platform packages, so
 * `@wae/open-cam` resolves the addon without any environment variable.
 */
import { defineConfig } from '@wae/wae';

export default defineConfig({
    native: {
        crate: 'open-cam-napi',
        cdylib: 'open_cam_napi',
        targets: [
            {
                platform: 'win32',
                arch: 'x64',
                triple: 'x86_64-pc-windows-msvc',
                package: '@wae/open-cam-win32-x64',
                nodeFile: 'open-cam.win32-x64.node',
                dir: 'projects/packages/open-cam-win32-x64',
            },
            {
                platform: 'darwin',
                arch: 'x64',
                triple: 'x86_64-apple-darwin',
                package: '@wae/open-cam-darwin-x64',
                nodeFile: 'open-cam.darwin-x64.node',
                dir: 'projects/packages/open-cam-darwin-x64',
            },
            {
                platform: 'darwin',
                arch: 'arm64',
                triple: 'aarch64-apple-darwin',
                package: '@wae/open-cam-darwin-arm64',
                nodeFile: 'open-cam.darwin-arm64.node',
                dir: 'projects/packages/open-cam-darwin-arm64',
            },
            {
                platform: 'linux',
                arch: 'x64',
                triple: 'x86_64-unknown-linux-gnu',
                package: '@wae/open-cam-linux-x64',
                nodeFile: 'open-cam.linux-x64.node',
                dir: 'projects/packages/open-cam-linux-x64',
            },
            {
                platform: 'linux',
                arch: 'arm64',
                triple: 'aarch64-unknown-linux-gnu',
                package: '@wae/open-cam-linux-arm64',
                nodeFile: 'open-cam.linux-arm64.node',
                dir: 'projects/packages/open-cam-linux-arm64',
            },
        ],
    },
    packages: [
        {
            name: '@wae/open-cam-win32-x64',
            dir: 'projects/packages/open-cam-win32-x64',
            os: ['win32'],
            cpu: ['x64'],
            main: 'open-cam.win32-x64.node',
        },
        {
            name: '@wae/open-cam-darwin-x64',
            dir: 'projects/packages/open-cam-darwin-x64',
            os: ['darwin'],
            cpu: ['x64'],
            main: 'open-cam.darwin-x64.node',
        },
        {
            name: '@wae/open-cam-darwin-arm64',
            dir: 'projects/packages/open-cam-darwin-arm64',
            os: ['darwin'],
            cpu: ['arm64'],
            main: 'open-cam.darwin-arm64.node',
        },
        {
            name: '@wae/open-cam-linux-x64',
            dir: 'projects/packages/open-cam-linux-x64',
            os: ['linux'],
            cpu: ['x64'],
            main: 'open-cam.linux-x64.node',
        },
        {
            name: '@wae/open-cam-linux-arm64',
            dir: 'projects/packages/open-cam-linux-arm64',
            os: ['linux'],
            cpu: ['arm64'],
            main: 'open-cam.linux-arm64.node',
        },
    ],
    build: {
        ts: { packages: ['@wae/open-cam'] },
        outputs: ['projects/packages/open-cam/dist/cli.js', 'projects/packages/open-cam/dist/index.js'],
    },
    skills: {
        packages: [
            {
                dir: 'projects/packages/open-cam-skills/skills',
                packageJson: 'projects/packages/open-cam-skills/package.json',
                appPackageJson: 'projects/packages/open-cam/package.json',
                cliEntry: 'projects/packages/open-cam/dist/cli.js',
                commandName: 'open-cam',
            },
        ],
    },
});
