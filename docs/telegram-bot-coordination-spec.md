# Telegram Bot Coordination Spec

## 목적
`cokacdir`의 텔레그램 다중 봇(예: 클로드딘, 코덱스딘)이 같은 그룹 채팅에서 더 자연스럽고 안정적으로 협업하도록 만드는 운영/개발 명세입니다.

## 배경 문제
기존 구조에서는 같은 그룹 채팅 안에서 봇들이 사실상 직렬 처리되어:
- 한 봇이 끝나야 다른 봇이 본처리를 시작하는 느낌이 났고
- 병렬 작업 체감이 약했으며
- `;` 브로드캐스트 협업 시 두 봇이 모두 리더처럼 행동하는 문제가 있었습니다.

또한 Claude 경로에서는 숨은 MCP/tool schema 주입으로 인해 다음 에러가 발생했습니다.
- `tools.66.custom.input_schema: input_schema does not support oneOf, allOf, or anyOf at the top level`

## 이번 변경의 목표
1. Claude 경로의 숨은 MCP/tool schema 문제를 우회해 텔레그램 봇이 안정적으로 응답하게 한다.
2. 같은 그룹 채팅에서도 봇별로 작업 시작을 더 독립적으로 처리한다.
3. `;` 브로드캐스트 협업에서 코덱스딘을 리더, 클로드딘을 서브로 더 강하게 유도한다.
4. bot-to-bot 메시지에서는 서브 봇의 재위임/리더 행세를 줄인다.

## 적용된 변경

### 1) Claude MCP 강제 제한
파일:
- `src/services/claude.rs`

변경 내용:
- Claude CLI 실행 인자에 아래를 추가함
  - `--strict-mcp-config`
  - `--mcp-config {"mcpServers":{}}`

의도:
- Claude CLI가 로컬 플러그인/MCP를 자동 주입하면서 발생하던 숨은 tool schema 문제를 피하기 위함.

비고:
- `--bare`도 실험했으나 인증(`Not logged in`) 문제를 유발해 최종안으로 채택하지 않음.

### 2) 그룹 락 / 취소 토큰을 bot+chat 단위로 분리
파일:
- `src/services/telegram.rs`

변경 내용:
- 그룹 락을 `chat_id` 단독이 아니라 `chat_id + bot_key` 기준으로 분리
- `cancel_tokens`를 `ChatId` 기준이 아니라 `bot+chat` 문자열 키 기준으로 변경

의도:
- 같은 그룹 채팅에서도 서로 다른 봇이 보다 독립적으로 본처리를 시작할 수 있게 하기 위함.

기대 효과:
- 예전처럼 “앞 봇이 끝나야 뒤 봇이 진짜 시작하는 느낌”을 줄임.

### 3) 브로드캐스트 협업 역할 유도 강화
파일:
- `src/services/telegram.rs`

변경 내용:
- `;` 브로드캐스트 요청은 `is_broadcast=true`로 전달
- 브로드캐스트일 때 리더십 안내 문구를 더 강하게 주입
- 현재 기본 정책:
  - `@cokakbot` = 리더
  - `@deankanecodexbot` = 서브

의도:
- 두 봇이 동시에 PM처럼 행동하는 현상을 줄이고,
- 코덱스딘이 최종 통합을 맡고 클로드딘은 보조 분석/리서치 역할을 맡도록 유도.

### 4) bot-to-bot subordinate 규칙 강화
파일:
- `src/services/telegram.rs`

변경 내용:
- `[BOT MESSAGE]` 경로에서 subordinate 역할을 더 직접적으로 프롬프트에 주입
- 재위임 금지 / 리더 행세 금지 / 서브 결과만 반환하도록 안내 강화

의도:
- 서브 봇이 다시 다른 봇에게 역할을 나누거나,
- 스스로 최종 정리권을 주장하는 것을 줄이기 위함.

## 현재 한계
이번 변경은 **완전한 역할 강제 시스템**이 아니라, 다음 성격을 가집니다.
- 구조 일부는 코드 레벨
- 역할 인식은 아직 상당 부분 프롬프트 유도 기반

즉 아래 한계가 남아 있을 수 있습니다.
1. 클로드딘이 복잡한 맥락에서 다시 자기 역할을 재해석할 수 있음
2. 리더/서브 상태가 request 단위로 명시 저장되지는 않음
3. subordinate의 최종 답변 금지가 코드 레벨로 완전히 차단되지는 않음

## 다음 단계 제안
더 확실한 협업 구조를 원하면 다음을 구현해야 함.

### A. request별 leader state 저장
예:
- `chat_id`
- `request_id`
- `leader_bot`
- `mode = broadcast/direct`
- `status = active/done`

### B. 각 봇 호출에 role_context 강제 주입
- leader / subordinate를 시스템 상태에서 읽어 전달
- 더 이상 봇이 스스로 역할을 추론하지 않게 함

### C. subordinate 코드 레벨 제한
- 재위임 금지
- 최종 사용자 통합 답변 금지
- leader에게만 결과 전달

## 운영 메모
- 현재 운영 반영 바이너리는 로컬 빌드본으로 교체되어 있음
- 로컬 롤백은 `/usr/local/bin/cokacdir.bak-*` 백업으로 가능
- GitHub 포크 반영 커밋:
  - `810b5ae` — `fix: stabilize telegram bot coordination and claude mcp config`

## 권장 운영 방침
1. 현재 버전은 유지하면서 실제 사용에서 협업 품질을 관찰한다.
2. 문제가 반복되면 request별 leader state 저장 방식으로 진짜 구조화를 진행한다.
3. upstream 반영이 필요하면 이 문서를 바탕으로 issue/PR 설명 자료로 활용한다.
