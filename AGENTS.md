# TailView 작업 지침

- Rust/egui 데스크톱 이미지 뷰어. 소스와 Cargo 프로젝트는 `app/`에 있다.
- 앱 이름은 `TailView`, Cargo 패키지와 실행 파일 이름은 `tailview`다.
- 포인트 컬러는 여우 꼬리색 `#F47A32`. V자 여우 꼬리 아이콘은 `app/assets/tailview.png`와 `.ico`다.
- Ponytail full: 기존 코드·표준 라이브러리·네이티브 기능을 재사용하고 최소 변경으로 원인을 해결한다.
- 불필요한 추상화·의존성·기능을 추가하지 않는다. 요청 없이는 하위 에이전트를 생성하지 않는다.
- 관련 부분만 `rg`로 검색하고, 독립적인 조회는 묶는다. 답변은 자연스럽고 간결하게 쓴다.
- 확대·축소와 웹툰 수치는 `app/MECHANICS.md`와 `app/mechanics.json`을 참고한다. 출처와 추정 여부를 보존한다.
- 비자명한 동작 변경에는 작은 회귀 테스트를 추가한다. 이름·색상 변경만을 위한 테스트는 추가하지 않는다.
- 검증: `cargo test --locked --manifest-path app/Cargo.toml`. 실행 파일 변경은 `cargo build --locked --manifest-path app/Cargo.toml`도 확인한다.
- Windows 아이콘은 `app/build.rs`가 Windows SDK 리소스 컴파일러로 삽입한다. PNG를 바꾸면 ICO도 함께 갱신한다.
- 광고·추적·불필요한 네트워크 기능을 넣지 않는다. 사용자 파일과 기존 분석 자료를 보존한다.
- `target/`, 로컬 분석 자료, 샘플, 로그, 비밀 정보는 커밋하지 않는다.
- 작업 범위에 맞게 한 번 검증하고 종료한다. 검증하지 않은 플랫폼과 제한은 명시한다.
