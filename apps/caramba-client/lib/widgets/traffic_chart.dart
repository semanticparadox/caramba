import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import 'package:caramba_client/data/models/traffic_point.dart';
import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// График трафика (fl_chart) из ряда [TrafficPoint] (`/app/traffic`).
///
/// Нейтральная заливка под линией, цвет — text.hi (без статус-цвета: это не
/// индикатор состояния подключения). Пустой ряд рисует плоскую подпись.
class TrafficChart extends StatelessWidget {
  final List<TrafficPoint> points;
  const TrafficChart({required this.points, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final box = Container(
      height: 120,
      padding: const EdgeInsets.fromLTRB(
        AppSpace.s2,
        AppSpace.s4,
        AppSpace.s4,
        AppSpace.s3,
      ),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      child: points.isEmpty
          ? Center(
              child: Text(
                'Нет данных о трафике',
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
            )
          : _line(c),
    );
    return box;
  }

  Widget _line(AppColors c) {
    final spots = <FlSpot>[
      for (var i = 0; i < points.length; i++)
        FlSpot(i.toDouble(), points[i].totalMb),
    ];
    return LineChart(
      LineChartData(
        gridData: const FlGridData(show: false),
        titlesData: const FlTitlesData(show: false),
        borderData: FlBorderData(show: false),
        lineTouchData: const LineTouchData(enabled: false),
        minX: 0,
        maxX: (points.length - 1).toDouble(),
        minY: 0,
        lineBarsData: [
          LineChartBarData(
            spots: spots,
            isCurved: true,
            barWidth: 2,
            color: c.textHi,
            dotData: const FlDotData(show: false),
            belowBarData: BarAreaData(
              show: true,
              color: c.textHi.withValues(alpha: 0.08),
            ),
          ),
        ],
      ),
    );
  }
}
