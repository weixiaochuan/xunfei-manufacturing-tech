# Question Bank Service

This folder contains the Summary3 question-bank code that belongs on the cloud
side of the product.

Imported layout:

- `system/`: full question-bank service implementation from
  `汇总3/firstwork/files.v21_最终/question_bank_system`.
- `INTERFACE_对接文档.md`: integration contract for wiring the service into the
  account/classroom flow.
- `AGENTS.md`: original module notes retained for future development.

Local-only companion tooling was placed under `tools/question-bank/` so it does
not become part of the normal desktop runtime or package by accident.

Isolation rules:

- Student exercise records, wrong-answer records, recommendations, and class
  statistics must be scoped by the authenticated account and class context.
- Student-facing question APIs must not return answers before submission.
- The desktop app and plugins must not pass a trusted `student_id` directly;
  the cloud service should derive it from the session.
