import React, { useState, useEffect } from 'react';
import {
  Card,
  Steps,
  Button,
  Space,
  Typography,
  Row,
  Col,
  Alert,
  Progress,
  Spin,
  App,
  Divider,
  Tag,
  Tooltip,
  InputNumber,
} from 'antd';
import {
  SettingOutlined,
  GatewayOutlined,
  DesktopOutlined,
  CheckCircleOutlined,
  RocketOutlined,
  InfoCircleOutlined,
  CloudServerOutlined,
  NodeIndexOutlined,
} from '@ant-design/icons';
import { apiGet, apiPost, apiAll, fetchWithTimeout } from '../utils/api';

const { Title, Paragraph, Text } = Typography;
const { Step } = Steps;

const InitialConfiguration = () => {
  const { message } = App.useApp();
  const [currentStep, setCurrentStep] = useState(0);
  const [systemStatus, setSystemStatus] = useState({
    infrastructure: false,
    controllers: false,
    dashboard: false,
    gateways: 0,
    devices: 0,
  });
  const [loading, setLoading] = useState(false);
  const [configuring, setConfiguring] = useState(false);
  const [gatewayCount, setGatewayCount] = useState(1);
  const [deviceCount, setDeviceCount] = useState(3);

  useEffect(() => {
    checkSystemStatus();
  }, []);

  const checkSystemStatus = async () => {
    setLoading(true);
    console.log('[InitialConfiguration] Starting system status check...');
    try {
      // Check infrastructure with timeout (increased to 10s)
      let infraStatus = false;
      try {
        console.log('[InitialConfiguration] Checking infrastructure at /api/v1/status...');
        const infraResponse = await fetchWithTimeout('/api/v1/status', {}, 10000);
        infraStatus = infraResponse.ok;
        console.log('[InitialConfiguration] Infrastructure status:', infraStatus, infraResponse.status);
      } catch (e) {
        console.error('[InitialConfiguration] Infrastructure check failed:', e);
        infraStatus = false;
      }

      // Check controllers by testing API endpoints with timeout (increased to 10s)
      let controllersStatus = false;
      try {
        console.log('[InitialConfiguration] Checking controllers (devices, applications, gateways)...');
        const [devicesResponse, applicationsResponse, gatewaysResponse] = await apiAll([
          { url: '/api/v1/devices', options: {} },
          { url: '/api/v1/applications', options: {} },
          { url: '/api/v1/gateways', options: {} }
        ], 10000);
        controllersStatus = true; // If we get here, all APIs responded
        console.log('[InitialConfiguration] Controllers status:', controllersStatus);
      } catch (e) {
        console.error('[InitialConfiguration] Controllers check failed:', e);
        controllersStatus = false;
      }

      // Check dashboard
      const dashboardStatus = true; // Dashboard is running

      // Check existing gateways and devices with timeout (increased to 10s)
      let gateways = [];
      let devices = [];
      try {
        console.log('[InitialConfiguration] Fetching gateways and devices...');
        const [gatewaysData, devicesData] = await apiAll([
          '/api/v1/gateways',
          '/api/v1/devices'
        ], 10000);
        
        gateways = gatewaysData.gateways || [];
        devices = devicesData.devices || [];
        console.log('[InitialConfiguration] Gateways:', gateways.length, 'Devices:', devices.length);
      } catch (e) {
        console.error('[InitialConfiguration] Failed to fetch gateways/devices:', e);
      }

      const newStatus = {
        infrastructure: infraStatus,
        controllers: controllersStatus,
        dashboard: dashboardStatus,
        gateways: gateways.length,
        devices: devices.length,
      };
      
      console.log('[InitialConfiguration] Setting system status:', newStatus);
      setSystemStatus(newStatus);

      // Auto-advance to next step if infrastructure is ready
      if (infraStatus && controllersStatus && dashboardStatus && currentStep === 0) {
        console.log('[InitialConfiguration] All systems ready, advancing to step 1');
        setCurrentStep(1);
      } else {
        console.log('[InitialConfiguration] Systems not ready yet:', {
          infraStatus,
          controllersStatus,
          dashboardStatus,
          currentStep
        });
      }
    } catch (error) {
      console.error('[InitialConfiguration] Error checking system status:', error);
      // Set default status on error
      setSystemStatus({
        infrastructure: false,
        controllers: false,
        dashboard: true,
        gateways: 0,
        devices: 0,
      });
    } finally {
      console.log('[InitialConfiguration] System status check completed');
      setLoading(false);
    }
  };

  const startControllers = async () => {
    setConfiguring(true);
    try {
      const result = await apiPost('/api/v1/terminal/execute', {
        command: 'kubectl get pods -n wasmbed'
      }, 10000);

      if (result.output.includes('No resources found')) {
        // Start controllers with timeout
        await apiPost('/api/v1/terminal/execute', {
          command: 'cd /home/lucadag/27_9_25_retrospect/retrospect && ./target/release/wasmbed-gateway-controller --kubeconfig ~/.kube/config &'
        }, 15000);
        
        await apiPost('/api/v1/terminal/execute', {
          command: 'cd /home/lucadag/27_9_25_retrospect/retrospect && ./target/release/wasmbed-device-controller --kubeconfig ~/.kube/config &'
        }, 15000);
        
        await apiPost('/api/v1/terminal/execute', {
          command: 'cd /home/lucadag/27_9_25_retrospect/retrospect && ./target/release/wasmbed-application-controller --kubeconfig ~/.kube/config &'
        }, 15000);
        
        message.success('Controllers started successfully!');
      } else {
        message.info('Controllers are already running');
      }
    } catch (error) {
      console.error('Error starting controllers:', error);
      message.error('Failed to start controllers');
    } finally {
      setConfiguring(false);
    }
  };

  const handleGatewayDeployment = async () => {
    setConfiguring(true);
    try {
      // Deploy multiple gateways with configurable count
      // Don't set endpoint - let the gateway controller set it automatically to Kubernetes service DNS
      const gatewayRequest = {
        count: gatewayCount, // Number of gateways to create
        // endpoint will be set automatically by gateway controller to {gateway-name}-service.wasmbed.svc.cluster.local:8080
        basePort: 30452
      };

      const result = await apiPost('/api/v1/gateways', gatewayRequest, 15000);
      message.success(result.message);
      await checkSystemStatus();
      setCurrentStep(2);
    } catch (error) {
      console.error('Error deploying gateways:', error);
      message.error('Error deploying gateways');
    } finally {
      setConfiguring(false);
    }
  };

  const handleDeviceDeployment = async () => {
    setConfiguring(true);
    try {
      // Deploy sample devices with configurable count
      const deviceRequest = {
        count: deviceCount, // Number of devices to create
        type: 'MCU', // ARM Cortex-M MCU for Renode emulation
        architecture: 'ARM_CORTEX_M',
        mcuType: 'RenodeArduinoNano33Ble', // Default to Arduino Nano 33 BLE
        gatewayId: 'gateway-1' // Will be updated to use the first available gateway
      };

      const result = await apiPost('/api/v1/devices', deviceRequest, 15000);
      message.success(result.message);
      await checkSystemStatus();
      setCurrentStep(3);
    } catch (error) {
      console.error('Error deploying devices:', error);
      message.error('Error deploying devices');
    } finally {
      setConfiguring(false);
    }
  };

  const steps = [
    {
      title: 'System Check',
      icon: <CloudServerOutlined />,
      description: 'Verifying infrastructure and controllers',
      content: (
        <div>
          <Row gutter={[16, 16]}>
            <Col span={8}>
              <Card size="small" title="Infrastructure">
                <div style={{ textAlign: 'center' }}>
                  {systemStatus.infrastructure ? (
                    <CheckCircleOutlined style={{ fontSize: 24, color: '#52c41a' }} />
                  ) : (
                    <Spin size="small" />
                  )}
                  <div style={{ marginTop: 8 }}>
                    <Text type={systemStatus.infrastructure ? 'success' : 'secondary'}>
                      {systemStatus.infrastructure ? 'Running' : 'Checking...'}
                    </Text>
                  </div>
                </div>
              </Card>
            </Col>
            <Col span={8}>
              <Card size="small" title="Controllers">
                <div style={{ textAlign: 'center' }}>
                  {systemStatus.controllers ? (
                    <CheckCircleOutlined style={{ fontSize: 24, color: '#52c41a' }} />
                  ) : (
                    <Spin size="small" />
                  )}
                  <div style={{ marginTop: 8 }}>
                    <Text type={systemStatus.controllers ? 'success' : 'secondary'}>
                      {systemStatus.controllers ? 'Running' : 'Checking...'}
                    </Text>
                  </div>
                </div>
              </Card>
            </Col>
            <Col span={8}>
              <Card size="small" title="Dashboard">
                <div style={{ textAlign: 'center' }}>
                  {systemStatus.dashboard ? (
                    <CheckCircleOutlined style={{ fontSize: 24, color: '#52c41a' }} />
                  ) : (
                    <Spin size="small" />
                  )}
                  <div style={{ marginTop: 8 }}>
                    <Text type={systemStatus.dashboard ? 'success' : 'secondary'}>
                      {systemStatus.dashboard ? 'Running' : 'Checking...'}
                    </Text>
                  </div>
                </div>
              </Card>
            </Col>
          </Row>
          <Divider />
          <Row gutter={[16, 16]}>
            <Col span={12}>
              <Alert
                message="System Status"
                description="All core services are running. You can now proceed to configure gateways and devices."
                type="success"
                showIcon
                style={{ marginTop: 16 }}
              />
            </Col>
            <Col span={12}>
              <Card size="small" title="Controller Management">
                <Button
                  type="primary"
                  icon={<SettingOutlined />}
                  onClick={startControllers}
                  loading={configuring}
                  block
                >
                  Start Controllers
                </Button>
                <div style={{ marginTop: 8, fontSize: 12, color: '#666' }}>
                  Automatically start Kubernetes controllers if not running
                </div>
              </Card>
            </Col>
          </Row>
        </div>
      ),
    },
    {
      title: 'Gateway Setup',
      icon: <GatewayOutlined />,
      description: 'Deploy and configure edge gateways',
      content: (
        <div>
          <Row gutter={[16, 16]}>
            <Col span={12}>
              <Card title="Gateway Configuration" size="small">
                <Paragraph>
                  Deploy your first gateway to handle device connections and application deployment.
                </Paragraph>
                <div style={{ marginBottom: 16 }}>
                  <Text strong>Gateway Count:</Text>
                  <InputNumber
                    min={1}
                    max={10}
                    value={gatewayCount}
                    onChange={setGatewayCount}
                    style={{ width: '100%', marginTop: 8 }}
                  />
                </div>
                <div style={{ marginBottom: 16 }}>
                  <Text strong>Default Configuration:</Text>
                  <ul style={{ marginTop: 8 }}>
                    <li>Names: gateway-1 to gateway-{gatewayCount}</li>
                    <li>Endpoints: 127.0.0.1:30452 to 127.0.0.1:{30452 + (gatewayCount - 1) * 2}</li>
                    <li>Max Devices: 50 per gateway</li>
                    <li>Region: us-west-1</li>
                  </ul>
                </div>
                <Button
                  type="primary"
                  icon={<RocketOutlined />}
                  onClick={handleGatewayDeployment}
                  loading={configuring}
                  disabled={systemStatus.gateways > 0}
                >
                  {systemStatus.gateways > 0 ? 'Gateway Already Deployed' : 'Deploy Gateway'}
                </Button>
              </Card>
            </Col>
            <Col span={12}>
              <Card title="Current Status" size="small">
                <div style={{ textAlign: 'center', padding: '20px 0' }}>
                  <div style={{ fontSize: 48, marginBottom: 16 }}>
                    {systemStatus.gateways > 0 ? (
                      <CheckCircleOutlined style={{ color: '#52c41a' }} />
                    ) : (
                      <GatewayOutlined style={{ color: '#1890ff' }} />
                    )}
                  </div>
                  <Title level={3} style={{ margin: 0 }}>
                    {systemStatus.gateways} Gateway{systemStatus.gateways !== 1 ? 's' : ''}
                  </Title>
                  <Text type="secondary">
                    {systemStatus.gateways > 0 ? 'Ready for devices' : 'Not deployed'}
                  </Text>
                </div>
              </Card>
            </Col>
          </Row>
        </div>
      ),
    },
    {
      title: 'Device Setup',
      icon: <DesktopOutlined />,
      description: 'Deploy sample devices for testing',
      content: (
        <div>
          <Row gutter={[16, 16]}>
            <Col span={12}>
              <Card title="Sample Devices" size="small">
                <Paragraph>
                  Deploy sample devices to test the platform functionality.
                </Paragraph>
                <div style={{ marginBottom: 16 }}>
                  <Text strong>Device Count:</Text>
                  <InputNumber
                    min={1}
                    max={20}
                    value={deviceCount}
                    onChange={setDeviceCount}
                    style={{ width: '100%', marginTop: 8 }}
                  />
                </div>
                <div style={{ marginBottom: 16 }}>
                  <Text strong>Sample Devices:</Text>
                  <ul style={{ marginTop: 8 }}>
                    <li>ARM Cortex-M4 MCU (Renode emulation)</li>
                    <li>Default: Arduino Nano 33 BLE (nRF52840)</li>
                    <li>Auto-generated names: device-1 to device-{deviceCount}</li>
                    <li>Connected to: gateway-1</li>
                  </ul>
                </div>
                <Button
                  type="primary"
                  icon={<RocketOutlined />}
                  onClick={handleDeviceDeployment}
                  loading={configuring}
                  disabled={systemStatus.devices > 0}
                >
                  {systemStatus.devices > 0 ? 'Devices Already Deployed' : 'Deploy Sample Devices'}
                </Button>
              </Card>
            </Col>
            <Col span={12}>
              <Card title="Current Status" size="small">
                <div style={{ textAlign: 'center', padding: '20px 0' }}>
                  <div style={{ fontSize: 48, marginBottom: 16 }}>
                    {systemStatus.devices > 0 ? (
                      <CheckCircleOutlined style={{ color: '#52c41a' }} />
                    ) : (
                      <DesktopOutlined style={{ color: '#1890ff' }} />
                    )}
                  </div>
                  <Title level={3} style={{ margin: 0 }}>
                    {systemStatus.devices} Device{systemStatus.devices !== 1 ? 's' : ''}
                  </Title>
                  <Text type="secondary">
                    {systemStatus.devices > 0 ? 'Ready for applications' : 'Not deployed'}
                  </Text>
                </div>
              </Card>
            </Col>
          </Row>
        </div>
      ),
    },
    {
      title: 'Complete',
      icon: <CheckCircleOutlined />,
      description: 'Configuration completed successfully',
      content: (
        <div style={{ textAlign: 'center', padding: '40px 0' }}>
          <CheckCircleOutlined style={{ fontSize: 64, color: '#52c41a', marginBottom: 24 }} />
          <Title level={2}>Configuration Complete!</Title>
          <Paragraph style={{ fontSize: 16, marginBottom: 32 }}>
            Your Wasmbed platform is now ready for use. You can start deploying applications
            and managing your edge devices.
          </Paragraph>
          <Row gutter={[16, 16]} justify="center">
            <Col>
              <Tag color="blue" style={{ fontSize: 14, padding: '8px 16px' }}>
                <GatewayOutlined /> {systemStatus.gateways} Gateway{systemStatus.gateways !== 1 ? 's' : ''}
              </Tag>
            </Col>
            <Col>
              <Tag color="green" style={{ fontSize: 14, padding: '8px 16px' }}>
                <DesktopOutlined /> {systemStatus.devices} Device{systemStatus.devices !== 1 ? 's' : ''}
              </Tag>
            </Col>
          </Row>
          <div style={{ marginTop: 32 }}>
            <Button
              type="primary"
              size="large"
              onClick={() => window.location.href = '/dashboard'}
            >
              Go to Dashboard
            </Button>
          </div>
        </div>
      ),
    },
  ];

  return (
    <div style={{ padding: '24px', maxWidth: '1200px', margin: '0 auto' }}>
      <div style={{ textAlign: 'center', marginBottom: 32 }}>
        <Title level={1} style={{ marginBottom: 8 }}>
          <SettingOutlined /> Initial Configuration
        </Title>
        <Paragraph style={{ fontSize: 16, color: '#666' }}>
          Set up your Wasmbed platform by configuring gateways and devices
        </Paragraph>
      </div>

      <Card>
        <Steps current={currentStep} size="small" style={{ marginBottom: 32 }}>
          {steps.map((step, index) => (
            <Step
              key={index}
              title={step.title}
              description={step.description}
              icon={step.icon}
            />
          ))}
        </Steps>

        <div style={{ minHeight: '400px' }}>
          {steps[currentStep].content}
        </div>

        <div style={{ textAlign: 'center', marginTop: 32 }}>
          <Space>
            {currentStep > 0 && (
              <Button onClick={() => setCurrentStep(currentStep - 1)}>
                Previous
              </Button>
            )}
            {currentStep < steps.length - 1 && (
              <Button
                type="primary"
                onClick={() => setCurrentStep(currentStep + 1)}
                disabled={currentStep === 0 && (!systemStatus.infrastructure || !systemStatus.controllers)}
              >
                Next
              </Button>
            )}
            <Button
              icon={<InfoCircleOutlined />}
              onClick={checkSystemStatus}
              loading={loading}
            >
              Refresh Status
            </Button>
          </Space>
        </div>
      </Card>
    </div>
  );
};

export default InitialConfiguration;
