# Volume 28: Memory 形成与失效

## 1. 目标

Memory 要让 Kiana 长期变聪明，但不能让旧事实污染当前判断。

## 2. Memory Formation Pipeline

步骤：

1. collect candidate。
2. classify type。
3. attach evidence。
4. score confidence。
5. detect sensitivity。
6. propose memory。
7. accept/reject。
8. index。

## 3. Candidate 来源

- successful fix。
- repeated failure。
- user preference。
- project decision。
- command pattern。
- architecture rationale。
- release blocker。
- EDA gotcha。

## 4. Type 分类

- observation。
- reasoning。
- git。
- preference。
- warning。
- command。
- architecture。
- blocker。

## 5. Confidence

| 等级 | 条件 |
| --- | --- |
| high | 有 live evidence + user confirmation |
| medium | 有 evidence 但未长期验证 |
| low | 推断 |
| stale | 可能过期 |

Low 不自动注入。

## 6. Evidence Binding

Memory 必须绑定：

- event id。
- file path。
- command。
- review。
- user decision。

无 evidence 的 memory 只能是 draft。

## 7. Stale Detection

触发：

- referenced file changed。
- command no longer exists。
- tests changed。
- workflow completed long ago。
- user contradicts。
- dependency changed。

## 8. Invalidation

失效行为：

- mark stale。
- remove from auto context。
- keep searchable。
- require refresh before claim。

## 9. Retrieval

检索排序：

- relevance。
- confidence。
- freshness。
- source quality。
- task match。

## 10. Memory Injection

注入规则：

- high confidence allowed。
- medium summarized with caveat。
- low omitted。
- stale only under historical section。

## 11. Memory Conflict

冲突来源：

- memory vs file。
- memory vs test。
- memory vs user。
- memory vs newer memory。

解决：

- live evidence wins。
- newer verified wins。
- user explicit wins if safe。

## 12. Privacy

不要记：

- secrets。
- private credentials。
- sensitive personal data。
- temporary tokens。

## 13. Memory Report

`/memory status` 应显示：

- entries count。
- stale count。
- high confidence count。
- recent proposals。
- rejected count。

## 14. 验收

P1 验收：

- memory proposal 有 evidence。
- stale memory 不进入执行上下文。
- conflict 能解释。
- user preference 可保留。
- release 状态需要 live verify。
