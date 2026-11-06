# TailView

공통 작업 지침은 [AGENTS.md](AGENTS.md)를 읽고 따른다.

Rust + eframe/egui 이미지·만화·웹툰 뷰어이며 Cargo 프로젝트는 `app/`에 있다.
브랜드는 TailView, 실행 파일은 `tailview`, 포인트 컬러는 `#F47A32`, 아이콘은 V자 여우 꼬리다.

```sh
cargo run --manifest-path app/Cargo.toml
cargo test --locked --manifest-path app/Cargo.toml
cargo build --locked --manifest-path app/Cargo.toml
```

최소한의 정확한 변경을 우선한다. 요청한 범위에서 구현·검증을 끝내고,
실제 검증 결과와 남은 제한을 짧게 보고한다. 기능과 설치 안내는 [README.md](README.md)를 참고한다.
