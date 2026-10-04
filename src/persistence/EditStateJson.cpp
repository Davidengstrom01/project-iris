#include "persistence/EditStateJson.h"

namespace iris {

QJsonObject adjustmentsToJson(const BasicAdjustments& adjustments)
{
    BasicAdjustments copy = adjustments;
    QJsonObject json;
    for (const AdjustmentField& field : adjustmentFields())
        json.insert(QLatin1String(field.key), double(field.value(copy)));
    return json;
}

void readAdjustments(const QJsonObject& json, BasicAdjustments& adjustments)
{
    for (const AdjustmentField& field : adjustmentFields()) {
        const QJsonValue value = json.value(QLatin1String(field.key));
        if (value.isDouble())
            field.value(adjustments) = float(value.toDouble());
    }
    adjustments = sanitized(adjustments);
}

} // namespace iris
