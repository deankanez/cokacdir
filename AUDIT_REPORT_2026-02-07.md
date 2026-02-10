# COKACDIR v0.4.22 프로덕션 감사 보고서 — 실제 버그만

**프로젝트**: COKACDIR (Dual-panel Terminal File Manager)
**버전**: 0.4.22
**조사일**: 2026-02-07
**조사 방법**: 14개 병렬 에이전트 3라운드 전수조사 → 검증 에이전트 확인 → 필터링 (개선사항 제거, 실제 버그만 선별)

---

## 요약

전체 코드베이스 (28개 .rs 파일, ~31,300 라인) 전수조사 결과, **실제로 크래시를 유발하거나 반드시 수정해야 하는 버그 3건**을 확인했습니다.

| 등급 | 건수 | 설명 |
|------|------|------|
| **HIGH** | 2 | 정상 사용에서 크래시 발생 |
| **MEDIUM** | 1 | HIGH 버그 발생 시 피해 확대 |

> 참고: 92건 이상의 지적사항을 최초 도출했으나, "개선되면 좋을 내용"과 "이론적 위험"을 모두 제거하고 **현재 구현상 실제로 깨지는 것만** 남겼습니다.

---

## BUG-1 [HIGH] UTF-8 바이트 슬라이싱 패닉

### 문제

Rust의 `&str[n..]` 또는 `&str[..n]`은 **바이트 인덱스**로 동작합니다. 한글은 한 글자가 3바이트이므로, `.len()` (바이트 길이)으로 계산한 인덱스가 글자 중간에 떨어지면 **즉시 패닉**합니다.

```
thread 'main' panicked at 'byte index 4 is not a char boundary; it is inside '나' (bytes 3..6)'
```

이 앱의 주 사용자가 한국어 사용자이고, 파일/폴더 이름에 한글이 포함되는 것은 일반적이므로, **정상 사용 중에도 충분히 발생**합니다.

### 발생 위치 (7곳)

#### 1) `src/ui/dialogs.rs:1547-1548` — 진행 다이얼로그 파일명 잘림

```rust
let max_filename_len = (inner.width - 8) as usize;
let current_file = if progress.current_file.len() > max_filename_len {
    format!("...{}", &progress.current_file[progress.current_file.len().saturating_sub(max_filename_len - 3)..])
```

**재현**: 한글 파일명이 포함된 디렉토리에서 복사/이동 진행 시, 터미널 폭이 좁으면 잘림 인덱스가 한글 바이트 중간에 위치 → 패닉.

#### 2) `src/ui/dialogs.rs:1769-1770` — tar 제외 경로 표시 잘림

```rust
let display_path = if path.len() > (inner.width - 6) as usize {
    format!("  ...{}", &path[path.len().saturating_sub((inner.width - 9) as usize)..])
```

#### 3) `src/ui/dialogs.rs:1872-1873` — 복사 제외 경로 표시 잘림

```rust
let display_path = if path.len() > (inner.width - 6) as usize {
    format!("  ...{}", &path[path.len().saturating_sub((inner.width - 9) as usize)..])
```

(2번과 동일한 패턴, 다른 다이얼로그에서 반복)

#### 4) `src/ui/search_result.rs:276-278` — 검색 결과 경로 잘림

```rust
let path_str = if path_display.len() > path_width {
    let skip = path_display.len() - path_width + 3;
    format!("{:.<width$}", &path_display[skip..], width = path_width)
```

**재현**: 한글 폴더 안에서 검색 시, 검색 결과의 상대 경로가 화면 폭보다 길면 패닉.

#### 5) `src/ui/dialogs.rs:1651-1652` — 충돌 다이얼로그 파일명 잘림

```rust
let truncated_name = if display_name.len() > max_name_len {
    format!("\"{}...\"", &display_name[..max_name_len.saturating_sub(4)])
```

**재현**: 한글 파일명이 긴 파일을 붙여넣기 시 파일 충돌 다이얼로그에서 패닉.

#### 6) `src/ui/ai_screen.rs:79-81` — AI 입력 길이 제한

```rust
const MAX_INPUT_LENGTH: usize = 4000;
if sanitized.len() > MAX_INPUT_LENGTH {
    sanitized.truncate(MAX_INPUT_LENGTH);  // 한글 바이트 경계에서 패닉
```

`String::truncate()`는 char boundary가 아닌 위치에서 호출하면 패닉합니다.

**재현**: AI 프롬프트에 한글을 포함해 4000바이트 이상 입력.

#### 7) `src/ui/dialogs.rs:1043-1044, 1261-1262` — 자동완성 접미사 추출

```rust
if selected_name.to_lowercase().starts_with(&current_prefix.to_lowercase()) {
    let suffix = &selected_name[current_prefix.len()..];
```

`to_lowercase()` 비교 후 원본의 바이트 길이로 슬라이싱합니다. `to_lowercase()`가 바이트 길이를 변경하는 문자(예: 독일어 ẞ→ß, 터키어 İ→i̇)에서 오프셋 불일치로 패닉 가능. 한글에는 해당하지 않으나, 동일한 안전하지 않은 패턴입니다.

### 추가: `src/ui/file_viewer.rs:949-962` — 검색 하이라이트 바이트 오프셋 불일치

```rust
let lower_line = line.to_lowercase();
let lower_term = search_term.to_lowercase();
for (byte_start, matched) in lower_line.match_indices(&lower_term) {
    let byte_end = byte_start + matched.len();
    // lower_line의 바이트 오프셋을 원본 line에 그대로 사용
    line[last_end..byte_start].to_string(),
    line[byte_start..byte_end].to_string(),
```

`lower_line.match_indices()`의 바이트 오프셋은 `lower_line`에 대한 것인데, 원본 `line`에 적용합니다. `to_lowercase()`가 바이트 길이를 변경하는 문자가 포함되면 인덱스 불일치로 패닉. 일반적인 한글/ASCII/CJK에서는 발생하지 않으나 특수 유니코드 문자에서 발생 가능합니다.

### 수정 방향

`.len()`과 바이트 인덱싱 대신 **char boundary를 존중하는 방식** 사용:

```rust
// 방법 1: char_indices() 사용
fn safe_truncate_end(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let skip = s.chars().count() - max_chars;
    let byte_idx = s.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    &s[byte_idx..]
}

// 방법 2: floor_char_boundary (nightly) 또는 수동 구현
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() { return s.len(); }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) { i -= 1; }
    i
}
```

`String::truncate()` 수정:
```rust
if sanitized.len() > MAX_INPUT_LENGTH {
    let boundary = floor_char_boundary(&sanitized, MAX_INPUT_LENGTH);
    sanitized.truncate(boundary);
}
```

`match_indices` 수정 — `lower_line` 대신 원본 `line`에서 직접 char 기반 매칭하거나, byte offset 매핑 테이블 사용.

---

## BUG-2 [HIGH] 심볼릭 링크 루프 시 스택 오버플로

### 문제

두 함수가 심볼릭 링크를 따라가며 재귀 탐색하는데, 루프 감지가 없어 **무한 재귀 → 스택 오버플로 → 프로세스 강제 종료**됩니다.

### 발생 위치

#### 1) `src/ui/file_info.rs:111-145` — 디렉토리 크기 계산

```rust
fn calculate_dir_size_recursive(path: &Path, cancel_flag: &AtomicBool) -> DirCalcResult {
    for entry in entries.filter_map(|e| e.ok()) {
        if let Ok(metadata) = entry.metadata() {  // ← 심볼릭 링크를 따라감
            if metadata.is_dir() {
                let sub_result = calculate_dir_size_recursive(&entry_path, cancel_flag);  // ← 무한 재귀
```

`entry.metadata()`는 심볼릭 링크를 따라가므로, 심볼릭 링크가 자기 부모 디렉토리를 가리키면 무한 루프.

#### 2) `src/ui/search_result.rs:100-156` — 재귀 검색

```rust
pub fn recursive_search(base_path: &PathBuf, current_path: &PathBuf, ...) {
    let metadata = match entry.metadata() {  // ← 심볼릭 링크를 따라감
        Ok(m) => m,
    };
    if is_directory {
        recursive_search(base_path, &path, search_term, results, max_results);  // ← 무한 재귀
```

`max_results` 제한이 있으나 (기본 1000), 심볼릭 링크 루프가 파일이 없는 디렉토리를 가리키면 결과가 증가하지 않으므로 탈출 조건에 도달하지 못합니다.

### 재현

```bash
mkdir -p /tmp/test_dir
ln -s /tmp/test_dir /tmp/test_dir/loop_link
# cokacdir에서 /tmp/test_dir 선택 후 'i' (파일 정보) 또는 검색 실행
```

### 수정 방향

```rust
// 방법 1: entry.symlink_metadata() 사용 (심볼릭 링크를 따라가지 않음)
if let Ok(metadata) = entry.symlink_metadata() {
    if metadata.is_symlink() {
        continue;  // 또는 별도 처리
    }
}

// 방법 2: 깊이 제한 추가
fn calculate_dir_size_recursive(path: &Path, cancel_flag: &AtomicBool, depth: u32) -> DirCalcResult {
    if depth > 100 { return DirCalcResult::default(); }
    // ...
    calculate_dir_size_recursive(&entry_path, cancel_flag, depth + 1);
}

// 방법 3: 방문한 inode 추적 (HashSet<(dev, ino)>)
```

---

## BUG-3 [MEDIUM] 패닉 훅 미설정 — 터미널 상태 미복구

### 문제

`src/main.rs`의 `main()` 함수에 `std::panic::set_hook()`이 없습니다.

BUG-1의 패닉이 발생하면:
1. raw mode가 해제되지 않음
2. alternate screen에서 빠져나오지 않음
3. 마우스 캡처가 해제되지 않음

결과: **터미널이 사용 불가 상태**가 됩니다. 사용자는 `reset` 명령을 타이핑하거나(보이지 않지만) 터미널을 강제 종료해야 합니다.

### 현재 코드 (`src/main.rs:175-189`)

```rust
// Setup terminal
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
// ...
// 정상 종료 시에만 복구됨 (222-232행)
disable_raw_mode()?;
execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, ...)?;
```

패닉 시 222-232행의 복구 코드에 도달하지 못합니다.

### 수정 방향

`enable_raw_mode()` 직전에 패닉 훅 설정:

```rust
fn main() -> io::Result<()> {
    // ... argument parsing ...

    // 패닉 시 터미널 복구
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::Show
        );
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    // ...
}
```

---

## 버그 간 관계

```
BUG-1 (UTF-8 패닉) ──발생──→ 프로세스 패닉
                                    │
                              BUG-3 (훅 없음)
                                    │
                              터미널 사용 불가

BUG-2 (심링크 루프) ──발생──→ 스택 오버플로 → 프로세스 강제 종료 (SIGABRT)
                              ※ BUG-3으로도 복구 불가 (패닉이 아닌 abort)
```

BUG-3을 수정하면 BUG-1으로 인한 터미널 망가짐은 방지됩니다. 그러나 BUG-2는 스택 오버플로로 abort되므로 패닉 훅으로도 잡을 수 없습니다. BUG-1과 BUG-2는 각각 근본 수정이 필요합니다.

---

## 우선순위

| 순위 | 버그 | 이유 |
|------|------|------|
| 1 | BUG-3 | 가장 간단한 수정이면서 BUG-1의 피해를 즉시 줄임 |
| 2 | BUG-1 | 한글 사용자에게 가장 빈번하게 발생하는 크래시 |
| 3 | BUG-2 | 심볼릭 링크 루프가 있는 디렉토리에서만 발생 |

---

## 제외된 항목에 대한 참고

최초 조사에서 도출된 92건 이상의 지적사항 중 89건은 다음과 같은 이유로 제외되었습니다:

- **개선 사항**: 코드 품질을 높이지만, 현재 동작이 잘못된 것은 아님 (예: unwrap() 사용, 에러 메시지 개선)
- **이론적 위험**: 실제 사용에서 트리거될 수 없거나 OS/라이브러리가 이미 보호하는 경우 (예: u64 오버플로는 18EB 파일 필요)
- **설계 선택**: 버그가 아닌 의도적 결정 (예: tokio를 비동기에만 사용, thread::spawn 병행 사용)
- **UI-level 보호**: 코드 수준에서는 취약해 보이나 UI 흐름이 해당 경로 진입을 차단 (예: 같은 디렉토리 붙여넣기)
