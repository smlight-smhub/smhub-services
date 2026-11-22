# Contributing

Fixes, translations, tests and improvements are welcome. This page says how
to send them and what SMLIGHT needs from you first.

## Before you start

- Read [README.md](README.md) and [AGENTS.md](AGENTS.md). AGENTS.md is written
  for AI coding agents and holds for people too: the commands, the rules and
  what "done" means are the same.
- Updates of this software reach hubs that are in people's homes. The one hard
  requirement is that an update never breaks an existing user's setup, and
  that going back to the previous version works
  ([docs/vision.md](docs/vision.md), "What must not break").
- For anything larger than a small fix, open an issue first and describe what
  you want to change and why. That saves you work if the change does not fit.

## The contributor agreement

Before SMLIGHT can accept your first pull request, you sign the
[Individual Contributor License Agreement](CLA.md), once. You sign it for
yourself; it cannot be signed on behalf of a company. If your employer has
rights in what you write, ask for its permission before you contribute. The
agreement is with Shenzhen Baishi Video Technology Co., Ltd.

**Why:** this software is licensed for noncommercial use, and SMLIGHT also
licenses it commercially. The agreement lets SMLIGHT include your work in
both. You keep the copyright in your contributions.

**How:**

1. Open your pull request.
2. The CLA assistant bot comments on it with a link to the agreement.
3. Follow the link and sign in with your GitHub account.
4. Read the agreement, fill in your name and email, and select **I agree**.
5. The `license/cla` check on your pull request turns green and the review
   can start.

A pull request whose author has not signed cannot be merged. You sign once;
later pull requests are not asked again.

Signing goes through [CLA assistant](https://cla-assistant.io), a free
service run by SAP. It records your GitHub account, the time you signed and
the details you enter, and SMLIGHT can see and export that record.

## Making a change

1. Fork the repository and make a branch for one topic.
2. Make the change, with tests next to the code it changes.
3. Run the checks in [AGENTS.md](AGENTS.md#commands) that apply: the Rust tests
   and clippy, the UI type check and build, and the translation check when you
   change texts.
4. Update the documents that describe the behaviour you changed.
5. Open a pull request that says what was wrong or missing, what the change
   does, how you checked it (commands, and on which hardware), and what you
   could not check.

Keep pull requests small. A change to the API starts in
[api/openapi.yaml](api/openapi.yaml); a new dependency needs a reason in
[docs/decisions.md](docs/decisions.md).

## What cannot be accepted

- Code you did not write yourself, unless you say where it comes from and
  under which license, and that license allows it here (see
  [AGENTS.md](AGENTS.md#license)).
- Secrets, real addresses, serial numbers, names or other private data, in
  code, docs, tests or screenshots.
- Changes that alter a stored format or a file that existing hubs rely on,
  without a way back to the previous version.

## Security issues

Do not report a security issue in a public issue. Use GitHub's private
vulnerability reporting: the repository's **Security** tab, then **Report a
vulnerability**.

## License of contributions

Your contributions are published under this repository's license
([LICENSE.md](LICENSE.md)) and licensed to SMLIGHT under the contributor
agreement you signed.
