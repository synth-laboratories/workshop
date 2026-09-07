# Provider policy

- Use `openai/gpt-5.6-luna` through OpenRouter for QA reviews unless the user explicitly changes the model.
- Never use Claude or Anthropic models. This is an explicit user requirement.
- Use authorized project-local environment files or the secrets proxy; never access Keychain.
