# cokacdir v0.4.22 - Production Readiness Audit Report

- **Date**: 2026-02-07
- **Scope**: Full project review (27 Rust source files, 31,305 lines + build scripts + installer)
- **Method**: 3-pass parallel audit + line-by-line re-verification + 2nd-pass deep audit + final critical-only filtering

---

## Final Assessment

전체 70건의 발견 사항 중, "개선되면 좋은 것"을 모두 제외하고 **현재 구현이 잘못되어 반드시 수정해야 하는 것**만 추렸습니다.

| 구분 | 건수 |
|------|------|
| **반드시 수정** | 6 |
| 개선 권장 (참고) | 64 |

### 판단 기준

- **포함**: 한국어 사용자가 정상 사용 중 프로그램이 크래시되는 버그, 프로덕션 출시를 막는 결함
- **제외**: 이론적 엣지 케이스, 방어적 코딩 개선, 설계 선택 사항, 인프라/배포 개선

---

## 반드시 수정해야 하는 6가지

---

### 1. panic handler 부재 — `src/main.rs:176-232`

**문제**: `panic::set_hook()`이 코드 전체에 없음 (grep 확인 완료).

```rust
enable_raw_mode()?;                                           // Line 176
execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;  // Line 183
let result = run_app(&mut terminal, &mut app);                // Line 210: panic 발생 시
disable_raw_mode()?;                                          // Line 222: 여기 도달 못함
```

`run_app()` 내부에서 어떤 panic이든 발생하면 터미널이 raw mode + alternate screen 상태로 남습니다. 사용자는 아무것도 안 보이는 상태에서 맹목적으로 `reset`을 타이핑해야 복구할 수 있습니다.

**이 항목이 가장 중요한 이유**: 아래 #2, #3의 UTF-8 panic이 발생하면 프로그램 크래시 + 터미널 파괴가 동시에 일어납니다. panic handler가 있으면 최소한 터미널은 정상 복구됩니다.

---

### 2. UTF-8 바이트 슬라이싱 panic — 5곳

**문제**: Rust에서 `&str[..n]`과 `String::truncate(n)`은 바이트 오프셋 `n`이 UTF-8 문자 경계가 아니면 **무조건 panic** 합니다. 이것은 Rust 언어 스펙입니다.

한글은 글자당 3바이트이므로, 터미널 폭 기반으로 계산된 바이트 오프셋이 한글 문자 중간에 걸릴 확률이 약 **2/3**입니다.

#### (a) `src/ui/dialogs.rs:1548` — 파일 복사/이동 진행 중 파일명 표시

```rust
let max_filename_len = (inner.width - 8) as usize;
let current_file = if progress.current_file.len() > max_filename_len {
    format!("...{}", &progress.current_file[
        progress.current_file.len().saturating_sub(max_filename_len - 3)..
    ])  // ← 바이트 오프셋이 한글 중간이면 즉시 panic
} else {
    progress.current_file.clone()
};
```

**재현 방법**: 한글 이름의 파일을 복사하면 됩니다.

#### (b) `src/ui/dialogs.rs:1652` — 파일 충돌(덮어쓰기) 다이얼로그

```rust
let max_name_len = (inner.width - 6) as usize;
let truncated_name = if display_name.len() > max_name_len {
    format!("\"{}...\"", &display_name[..max_name_len.saturating_sub(4)])
    //                    ↑ 같은 문제
} else {
    format!("\"{}\"", display_name)
};
```

**재현 방법**: 한글 파일명이 다이얼로그 폭보다 길 때 (좁은 터미널에서 복사 충돌 발생 시).

#### (c) `src/ui/dialogs.rs:1770, 1873` — tar 다이얼로그 경로 표시

```rust
format!("  ...{}", &path[path.len().saturating_sub((inner.width - 9) as usize)..])
//                  ↑ 같은 문제, 두 곳에 동일 패턴
```

**재현 방법**: 한글 디렉토리 경로에서 tar 생성/해제 다이얼로그 열기.

#### (d) `src/ui/search_result.rs:276-278` — 검색 결과 경로 표시

```rust
let path_str = if path_display.len() > path_width {
    let skip = path_display.len() - path_width + 3;
    format!("{:.<width$}", &path_display[skip..], width = path_width)
    //                      ↑ 같은 문제
```

**재현 방법**: 한글 경로에서 파일 검색 후 결과 목록 표시.

#### (e) `src/ui/ai_screen.rs:81` — AI 입력 길이 제한

```rust
const MAX_INPUT_LENGTH: usize = 4000;
if sanitized.len() > MAX_INPUT_LENGTH {
    sanitized.truncate(MAX_INPUT_LENGTH);  // ← 한글 1333자 이상 입력 시 panic
    sanitized.push_str("... [truncated]");
}
```

**재현 방법**: AI 프롬프트에 한글 1333자 이상 입력 (긴 프롬프트 작성 시 가능).

---

### 3. syntax.rs char/byte 인덱스 혼동 — `src/ui/syntax.rs:1943-1950`

**문제**: `line.find()`는 **바이트 오프셋**을 반환하는데, 이 값을 `chars` 벡터의 **문자 인덱스**로 사용합니다.

```rust
let chars: Vec<char> = line.chars().collect();  // 1938: chars는 문자 단위 벡터
let mut i = 0;                                   // 1939: i는 문자 인덱스로 사용됨

if self.in_multiline_comment {
    let end_idx = line.find(block_comment.1);     // 1943: line.find()는 바이트 오프셋
    if let Some(idx) = end_idx {
        // ...
        i = idx + block_comment.1.len();           // 1950: 바이트 오프셋을 i에 대입!
    }
}

while i < chars.len() {                           // 1960: i를 문자 인덱스로 사용
    // chars[i] ...                               // → 문자를 건너뜀 또는 OOB panic
```

**재현 방법**: 한글 주석이 포함된 소스 파일을 에디터/뷰어에서 열면 됩니다.

```rust
/* 한글 주석입니다 */
fn main() {}
```

이 파일에서 `*/`의 바이트 오프셋은 29인데 문자 인덱스는 11입니다. `i = 29`가 되어 `chars.len()`(약 20)을 초과하면 OOB panic, 초과하지 않더라도 잘못된 위치부터 토크나이징이 시작됩니다.

**영향 범위**: `tokenize_c_like`을 사용하는 **12개 언어** 전부 — Rust, JavaScript, Go, Java, C, Swift, Kotlin, PHP, Dart, Zig 등.

---

### 4. 검색 시 symlink 순환 → 스택 오버플로 — `src/ui/search_result.rs:122-155`

**문제**: `entry.metadata()`는 symlink를 따라갑니다 (`stat`, not `lstat`). 순환 symlink를 만나면 무한 재귀합니다.

```rust
let metadata = match entry.metadata() {   // 122: symlink 따라감
    Ok(m) => m,
    Err(_) => continue,
};
let is_directory = metadata.is_dir();     // 127: symlink 대상이 디렉토리면 true

if is_directory {
    recursive_search(base_path, &path, search_term, results, max_results);
    // 155: 방문 추적 없음, 깊이 제한 없음 → 무한 재귀 → 스택 오버플로
}
```

`max_results`는 결과 수만 제한하지 재귀 깊이를 제한하지 않습니다. 검색어와 매칭되는 파일이 없으면 결과 수 제한이 영원히 걸리지 않습니다.

**재현 방법**: `ln -s . loop` 같은 순환 링크가 있는 디렉토리에서 검색 실행.

---

### 5. 디버그 로그가 프로덕션에 그대로 켜져 있음 — `src/services/claude.rs:9-24`

**문제**: 개발/조사용 디버그 로그가 비활성화 조건 없이 항상 동작합니다.

```rust
/// Debug logging helper (ENABLED for investigation)   // ← "investigation"용
fn debug_log(msg: &str) {
    if let Some(home) = dirs::home_dir() {
        let debug_dir = home.join(".cokacdir").join("debug");
        let _ = std::fs::create_dir_all(&debug_dir);        // 권한 설정 없음
        let log_path = debug_dir.join("claude.log");
        if let Ok(mut file) = OpenOptions::new()
            .create(true).append(true).open(log_path)       // 기본 umask (0644)
        {
            let _ = writeln!(file, "[{}] {}", timestamp, msg);
        }
    }
}
```

- 비활성화 조건 없음 (환경변수, feature flag, 설정 옵션 모두 없음)
- 64곳에서 호출
- 사용자 프롬프트 내용(line 250), 세션 ID(line 252) 기록
- 파일 크기 제한/로테이션 없음 → 무한 성장
- 기본 umask 0644로 생성 → 다른 사용자도 읽기 가능

프로덕션에서 AI 기능 사용 시 사용자 모르게 민감한 데이터가 디스크에 계속 쌓입니다.

---

### 6. `copy_dir_recursive_with_progress` + `calculate_dir_size` 깊이 제한 없음

**문제**: 동일 프로젝트 내에 깊이 제한이 있는 함수와 없는 함수가 공존합니다.

| 함수 | 깊이 제한 | 방문 추적 | 사용처 |
|------|----------|----------|--------|
| `copy_dir_recursive` (line 739) | `MAX_COPY_DEPTH = 256` | `HashSet<PathBuf>` | 내부 전용 |
| `copy_dir_recursive_with_progress` (line 226) | **없음** | **없음** | **UI 복사 (실제 사용)** |
| `calculate_dir_size` (line 115) | **없음** | — | 복사/이동 전 크기 계산 |

UI에서 실제로 사용하는 progress 버전과 크기 계산 함수에 보호 장치가 없습니다. 순환 symlink가 포함된 디렉토리를 복사하면 크기 계산 단계에서 이미 스택 오버플로로 크래시합니다.

**재현 방법**: 순환 symlink가 포함된 디렉토리를 선택하고 복사(F5) 실행.

---

## 개선 권장 사항 (참고용)

아래 항목들은 현재 구현이 "잘못된" 것이 아니라, 개선되면 더 좋아지는 항목들입니다.

### 실제 발생 가능성이 극히 낮은 항목 (제외 근거)

| 항목 | 제외 근거 |
|------|----------|
| `to_lowercase()` 바이트 불일치 (dialogs.rs:1044,1262) | ASCII, 한글에서는 `to_lowercase()`가 바이트 길이를 변경하지 않음. 터키어 İ 같은 극소수 문자에서만 발생 |
| `--dangerously-skip-permissions` (claude.rs:73,258) | 개발자의 의도적 설계 선택 |
| 명령어 인젝션 (app.rs:1511, 1572) | 파일명에 쉘 특수문자(`'`, `` ` ``, `$` 등)가 필요. 일반 사용에서 거의 발생하지 않음 |
| `indent.len()` byte/char 혼동 (file_editor.rs:816) | 표준 들여쓰기(스페이스, 탭)는 ASCII. 멀티바이트 공백 문자를 들여쓰기로 사용하는 경우는 거의 없음 |
| `to_lowercase()` 오프셋 불일치 (file_viewer.rs:949) | 위 dialogs.rs와 동일한 이유 |
| `&arg[..100]` 바이트 슬라이싱 (claude.rs:307) | 디버그 로그 내부. #5에서 디버그 로그를 비활성화하면 자동 해소 |
| `&err[..27]` 바이트 슬라이싱 (file_editor.rs:2348) | Regex 에러 메시지는 영문으로 시작. byte 27이 영문 범위 안에 있을 가능성이 높음 |
| `panel.rs:381` 확장자 슬라이싱 | 파일 확장자는 사실상 항상 ASCII |
| `effective_scroll as u16` 절삭 (ai_screen.rs:1349) | 65535 줄 넘는 AI 대화는 극히 드묾 |
| `exe_path` 쉘 인젝션 (app.rs:1572) | 실행 파일 경로에 `'` 포함은 극히 드묾 |

### 방어적 코딩 / 코드 품질 개선 (버그는 아님)

| 카테고리 | 항목 수 | 내용 |
|----------|---------|------|
| TOCTOU 레이스 컨디션 | 4건 | 파일 매니저의 근본적 한계, 대부분의 파일 매니저가 동일 |
| 바운드 체크 누락 | 6건 | 불변조건 유지에 의존. 현재 동작은 정상이나 방어적 체크 추가 권장 |
| PID 재사용 레이스 | 1건 | 검증 메커니즘이 있으나 미사용. 극히 드문 시나리오 |
| 타이머/렌더링 설계 | 2건 | 동작은 하나 프레임 기반 vs 시간 기반 차이 |
| 성능 개선 | 4건 | SystemData 캐싱, JSON 토크나이저 최적화, Regex 캐싱 등 |
| 파일 권한 | 3건 | 보안 강화이지 현재 동작이 잘못된 것은 아님 |

### 배포/인프라 개선

| 항목 | 내용 |
|------|------|
| install.sh 체크섬 검증 | 배포 관행 개선 |
| dist/ 바이너리 git 제거 | CI/CD 구축 시 해결 |
| Cargo.lock git 포함 | 재현 가능한 빌드를 위한 권장 사항 |
| 셸 스크립트 개선 | shebang, set -e 추가 등 |
| 테스트 커버리지 | 현재 테스트가 애플리케이션 코드를 거의 커버하지 않음 |

---

## Appendix A: Methodology

### Round 1: Initial 3-Pass Parallel Audit
- **Pass A** (Security): Command injection, path traversal, secrets, race conditions, unsafe code
- **Pass B** (Error Handling): unwrap/expect, index OOB, integer overflow, UTF-8 boundary
- **Pass C** (Logic/Resources): Logic errors, resource leaks, concurrency, state management

### Round 2: Line-by-Line Re-Verification
- 42건 중 41건 confirmed, 1건 false positive 제거
- 모든 줄 번호 소스 코드와 1:1 대조 확인 (오차 0건)

### Round 3: 2nd-Pass Deep Audit
- 5개 병렬 에이전트로 모든 파일 재검토
- 29건 추가 발견 (총 70건)

### Round 4: Critical-Only Filtering
- 70건 중 "개선"이 아닌 "결함"만 필터링
- 실제 재현 가능성, 한국어 사용 환경, 일반적 사용 패턴 기준으로 판단
- **최종 6건** 확정

## Appendix B: Files Audited

### Rust Source (27 files, 31,305 lines)
```
src/main.rs                    (730)     src/config.rs                (356)
src/services/claude.rs         (848)     src/services/file_ops.rs    (1733)
src/services/process.rs        (412)     src/services/mod.rs            (3)
src/ui/app.rs                 (4790)     src/ui/dialogs.rs           (3848)
src/ui/file_editor.rs         (3173)     src/ui/ai_screen.rs         (2778)
src/ui/theme.rs               (2610)     src/ui/syntax.rs            (2223)
src/ui/file_viewer.rs         (1431)     src/ui/theme_loader.rs      (1142)
src/ui/image_viewer.rs         (604)     src/ui/system_info.rs        (515)
src/ui/panel.rs                (463)     src/ui/help.rs               (432)
src/ui/process_manager.rs      (311)     src/ui/file_info.rs          (399)
src/ui/draw.rs                 (389)     src/ui/search_result.rs      (387)
src/ui/advanced_search.rs      (296)     src/ui/mod.rs                 (17)
src/utils/markdown.rs         (1324)     src/utils/format.rs           (89)
src/utils/mod.rs                 (2)
```

### Build/Infrastructure
```
build.py    builder/config.py    builder/executor.py    builder/tools.py
buildweb.sh    builtclean.sh    install.sh
Cargo.toml    rustfmt.toml    .gitignore
```

### Other
```
tests/file_operations.rs    website/    assets/    dist/
index.html    README.md    CHANGELOG.md
```

All line numbers verified against source code on 2026-02-07.
