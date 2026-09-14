# Contributing to Tonic / Tonic'e Katkı

Thank you for helping improve Tonic. The project is in active alpha development,
so correctness, explicit invariants, and reproducible measurements take priority
over API stability.

Tonic'e katkı sağladığınız için teşekkürler. Proje aktif alfa geliştirme
aşamasındadır; doğruluk, açık invariant'lar ve tekrar üretilebilir ölçümler API
kararlılığından önce gelir.

## Development setup / Geliştirme ortamı

Requirements are Rust stable 1.86+, Cargo, a C11 compiler, and CPython 3.12+
development files for the optional CPython bridge.

Gereksinimler Rust stable 1.86+, Cargo, bir C11 derleyicisi ve isteğe bağlı
CPython köprüsü için CPython 3.12+ geliştirme dosyalarıdır.

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

## Before a pull request / Pull request öncesi

Run the checks below and include focused tests for semantic changes:

Semantik değişiklikler için odaklı testler ekleyin ve aşağıdaki kontrolleri
çalıştırın:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --release --workspace --locked
python3 tests/differential/run.py
TONIC_GC_EVERY=1 python3 tests/differential/run.py
TONIC_JIT=1 python3 tests/differential/run.py
TONIC_JIT=1 TONIC_GC_EVERY=1 python3 tests/differential/run.py
cc -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/c_header_smoke.c
```

Changes to hot paths should start from a stable benchmark, record the baseline,
change one major variable, and record the result. Keep an optimization only when
it has a measured benefit or is required by a documented later tier.

Sıcak yol değişikliklerinde önce kararlı bir benchmark ve baseline kaydedin,
tek bir ana değişkeni değiştirin ve sonucu yeniden ölçün. Ölçülmüş faydası veya
belgelenmiş sonraki bir tier gereksinimi olmayan karmaşıklığı korumayın.

## Architecture rules / Mimari kurallar

- Read [AGENTS.md](AGENTS.md) before changing runtime architecture.
- Keep CPython layouts and reference counting inside `tonic-cpython`.
- Preserve opaque logical handles and moving-GC safety.
- Verify bytecode before any unchecked execution path.
- Every JIT assumption needs a guard and a safe fallback/deoptimization path.
- Use guest exceptions for language errors; reserve Rust panics for internal bugs.
- Add or update an ADR for decisions that change a subsystem contract.

## Pull request content / Pull request içeriği

Describe the concrete problem, the resulting behavior, validation performed, and
any remaining limitation. Keep commits focused and do not include `target/`,
Python bytecode caches, editor state, credentials, or machine-local configuration.

Somut problemi, yeni davranışı, uygulanan doğrulamayı ve kalan sınırlamaları
açıklayın. Commit'leri odaklı tutun; `target/`, Python bytecode cache'leri,
editör durumu, kimlik bilgileri veya makineye özel ayarları eklemeyin.

## Conduct / Davranış

Be respectful, technical, and specific. Review ideas and evidence rather than
people. Harassment, discrimination, and disclosure of private information are
not accepted.

Saygılı, teknik ve somut iletişim kurun. Kişileri değil fikirleri ve kanıtları
değerlendirin. Taciz, ayrımcılık ve özel bilgilerin paylaşılması kabul edilmez.

## Licensing / Lisanslama

No open-source license has been selected yet. Do not assume that source
availability grants redistribution rights. Contribution licensing will be made
explicit before outside contributions are accepted.

Henüz bir açık kaynak lisansı seçilmemiştir. Kaynağın görünür olmasının yeniden
dağıtım hakkı verdiğini varsaymayın. Dış katkılar kabul edilmeden önce katkı
lisanslaması açıkça belirlenecektir.
