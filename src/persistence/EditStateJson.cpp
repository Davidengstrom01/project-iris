#include "persistence/EditStateJson.h"

#include <QJsonArray>

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

QJsonObject toneCurveToJson(const ToneCurve& curve)
{
    QJsonArray points;
    for (const CurvePoint& p : curve.rgb)
        points.append(QJsonArray{double(p.x), double(p.y)});
    return QJsonObject{{"points", points}};
}

std::optional<ToneCurve> readToneCurve(const QJsonValue& json)
{
    const QJsonValue points = json.toObject().value("points");
    if (!points.isArray())
        return std::nullopt;
    CurvePoints parsed;
    for (const QJsonValue& value : points.toArray()) {
        const QJsonArray pair = value.toArray();
        if (pair.size() != 2 || !pair[0].isDouble() || !pair[1].isDouble())
            return std::nullopt;
        parsed.push_back({float(pair[0].toDouble()), float(pair[1].toDouble())});
    }
    ToneCurve curve;
    curve.rgb = normalizedCurve(parsed);
    return curve;
}

QJsonObject hslToJson(const HslAdjustments& hsl)
{
    QJsonObject json;
    for (int i = 0; i < kHslColorCount; ++i) {
        const HslBand& band = hsl.bands[i];
        if (band == HslBand{})
            continue;
        json.insert(QLatin1String(hslColorKey(HslColor(i))),
                    QJsonObject{{"hue", double(band.hue)},
                                {"saturation", double(band.saturation)},
                                {"luminance", double(band.luminance)}});
    }
    return json;
}

HslAdjustments readHsl(const QJsonValue& json)
{
    HslAdjustments hsl;
    const QJsonObject object = json.toObject();
    for (int i = 0; i < kHslColorCount; ++i) {
        const QJsonObject band = object.value(QLatin1String(hslColorKey(HslColor(i)))).toObject();
        hsl.bands[i].hue = float(band.value("hue").toDouble(0));
        hsl.bands[i].saturation = float(band.value("saturation").toDouble(0));
        hsl.bands[i].luminance = float(band.value("luminance").toDouble(0));
    }
    return sanitized(hsl);
}

} // namespace iris
