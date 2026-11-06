<p align="center">
  <img src="app/assets/tailview.png" width="144" alt="V자 여우 꼬리 TailView 아이콘">
</p>

# TailView

Rust와 egui로 만든 광고 없는 이미지·만화·웹툰 뷰어입니다.
Windows와 macOS용 코드로 구성되어 있습니다. 여우 꼬리를 V자로 형상화한 아이콘과
폭스 오렌지(`#F47A32`) 포인트 컬러를 사용합니다.

## 다운로드

[GitHub Releases](https://github.com/BBQ-MAN/TailView/releases)에서 Windows x64 ZIP을 받아
압축을 푼 뒤 `tailview.exe`를 실행하세요. Windows 렌더러는 DirectX 12를 사용합니다.
[Microsoft Visual C++ x64 런타임](https://aka.ms/vc14/vc_redist.x64.exe)이 필요합니다.
릴리즈의 `SHA256SUMS.txt`로 다운로드 파일을 검증할 수 있습니다.

## 주요 기능

- 이미지·폴더·ZIP/CBZ 열기, 파일 끌어다 놓기, 자연 정렬
- 화면·폭·높이 맞춤, 두 장 보기, 웹툰 연속 스크롤
- 마우스 위치를 기준으로 확대·축소, 전체 화면, 슬라이드쇼
- 회전·반전·흑백·세피아·밝기 등 이미지 효과
- JPEG·PNG·WebP·BMP·TIFF 병렬 일괄 변환
- PSD 합성 이미지와 CLIP 내장 미리보기 읽기

JPEG, PNG, GIF, WebP, BMP, TIFF, TGA, ICO, HDR, EXR, PNM, QOI, DDS 등을 읽습니다.
HEIC, AVIF, RAW 등은 운영체제 코덱 설치 여부에 따라 달라집니다.
GIF/APNG는 정지 이미지로 표시하며, HDR 파일도 현재 SDR 밝기로 표시합니다.

## 빌드 및 실행

[Rust](https://www.rust-lang.org/tools/install)를 설치한 뒤 저장소 루트에서 실행합니다.
Windows에서는 MSVC Rust 도구 체인과 Visual Studio C++ Build Tools 및 Windows SDK가 필요합니다.
Windows SDK의 `rc.exe`는 자동 탐색하며 `RC` 환경 변수로 경로를 지정할 수도 있습니다.
macOS에서는 Xcode Command Line Tools가 필요합니다.

```sh
cargo run --manifest-path app/Cargo.toml
cargo run --manifest-path app/Cargo.toml -- "path/to/image.png"
cargo run --manifest-path app/Cargo.toml -- --webtoon "path/to/comic.cbz"
cargo build --release --manifest-path app/Cargo.toml
```

실행 파일은 Windows에서 `app/target/release/tailview.exe`, macOS에서
`app/target/release/tailview`입니다. Windows 실행 파일에는 아이콘과 TailView 제품 정보가 포함됩니다.
macOS `.app` 번들·설치 프로그램은 아직 제공하지 않습니다.

## 단축키

| 키 | 동작 |
| --- | --- |
| Ctrl/Cmd+O / F | 파일 / 폴더 열기 |
| ← / → | 이전 / 다음 이미지 |
| + / - | 확대 / 축소 |
| 0 / 9 / 8 | 원본 크기 / 화면 맞춤 / 폭 맞춤 |
| 7 / 6 | 두 장 보기, 좌→우 / 우→좌 |
| 5 | 웹툰 모드 |
| Space / PageDown / PageUp | 스크롤 또는 페이지 이동 |
| Ctrl/Cmd+1…9 / Ctrl/Cmd+0 | 1…9초 슬라이드쇼 / 중지 |
| F11 | 전체 화면 |
| F1 | 도움말 |

## 개발

```sh
cargo test --locked --manifest-path app/Cargo.toml
```

- `app/src/main.rs`: UI, 보기 모드, 확대·축소, 이미지 효과
- `app/src/decode.rs`: 파일·압축 파일 탐색과 디코딩
- `app/src/convert.rs`: 병렬 일괄 변환
- `app/assets/`: 아이콘 PNG·ICO와 Windows 리소스
- `app/MECHANICS.md`, `app/mechanics.json`: 기존 동작 수치의 출처와 추정값 기록

TailView는 BandiView와 별개의 프로젝트입니다. 기존 분석 문서의 BandiView 이름은
참조한 원본 프로그램을 가리킵니다. 분석 기록·로컬 미리보기·빌드 결과는 Git에 포함하지 않습니다.
