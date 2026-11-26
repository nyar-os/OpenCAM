# @wae/open-cam-skills

Agent instructions for OpenCAM: the command surface, the preflight steps and the
evidence rules an assistant needs to prepare a 2.5-axis job without inventing a
capability.

## Install

```bash
pnpm add @wae/open-cam-skills
```

Point your agent at the skill file:

```text
node_modules/@wae/open-cam-skills/skills/open-cam/SKILL.md
```

## Example prompt

> Create a 120 × 80 × 12 mm job, add a 6 mm end mill, pocket a 60 × 40 mm area
> with two islands at 3 mm depth, contour the outside in two passes, generate
> G-code, and export the toolpath drawing as SVG and PDF.

The skill tells the agent to run `open-cam capabilities` first, to report the
tool, depth and feed it actually used, and to state that the output is not a
collision-checked or rest-machining plan.

## Links

- [OpenCAM skill](./skills/open-cam/SKILL.md)
- [OpenCAM README](../../../README.md)
- [CLI reference](../../../documentation/cli.md)
- [Capabilities](../../../documentation/capabilities.md)
