% 데이터 생성
x = linspace(0, 2*pi, 100); % 0~2pi 사이 100개 점
y = sin(x);                % 사인값 계산

% 그래프 그리기
figure;
plot(x, y, 'LineWidth', 2);
title('Sinusoidal Wave');
xlabel('x');
ylabel('sin(x)');
grid on;
