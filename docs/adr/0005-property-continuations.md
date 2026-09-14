# ADR 0005 — Property data descriptor ve VM continuation

Durum: uygulanmış bootstrap. 1 Eylül 2026. Public stable ABI değildir.

Property optional getter/setter logical Value'larını taşır ve GC ikisini izler.
Instance okuması own slotlardan önce class/MRO property değerini arar. Getter normal
Tonic frame'ine girer. Setter receiver + value ile aynı binder'ı kullanır;
`ReturnAction::Setter` dönüşü None'a çevirir. Implicit receiver guest tuple/dict
üretmez. `@x.setter` yeni immutable property üretir; `fget/fset` okunabilir.

Normal attribute, iki argümanlı `getattr` ve `setattr` desteklenir. Guest exception
yakalama olmadığı için property üzerinde `hasattr` ve default'lu `getattr` açık
UnsupportedFeature verir. `getter/deleter`, custom descriptor/metaclass ve cache
yoktur. Ölçüm [Stage 5 raporundadır](../STAGE5_BENCHMARKS.md).
