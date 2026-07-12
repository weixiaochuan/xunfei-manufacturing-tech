# AI Learning Assistant Skill

## Goal

AI Learning Assistant turns a learner's goal into a staged, executable study plan.
It is designed as an external skill folder that Pomegranate can check and call,
similar to how the app calls `ppt-master`.

## When To Use

Use this skill when the user wants to:

- Clarify a learning goal.
- Convert a course target into a staged plan.
- Break each stage into study, resource, practice, and check tasks.
- Reserve future hooks for resources, tests, scores, progress records, and plan adjustment.

## Inputs

- Learning goal.
- Course name.
- Learning cycle.
- Daily study time.
- Current foundation.
- Final target.

## Outputs

- Structured goal understanding.
- 3-5 learning stages.
- For each stage: stage name, time arrangement, stage goal, learning tasks,
  resource tasks, practice tasks, check tasks, and completion criteria.

## Workflow

Run the MVP workflow in:

- `workflows/generate-learning-plan.md`

Apply the rules in:

- `references/planning-rules.md`
- `references/scoring-rules.md`

## Closed Loop

The complete learning loop is:

Goal analysis -> plan generation -> stage tasks -> resource recommendation -> outcome check -> progress record -> plan adjustment.

The MVP only implements:

Goal analysis -> plan generation -> stage task display.
