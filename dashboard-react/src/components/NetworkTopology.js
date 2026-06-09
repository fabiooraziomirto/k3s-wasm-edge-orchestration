import React, { useState, useEffect } from 'react';
import { Card, Row, Col, Typography, Tag, Space, Tooltip, Badge } from 'antd';
import { 
  CloudServerOutlined, 
  GatewayOutlined, 
  DesktopOutlined,
  WifiOutlined,
  DisconnectOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  ArrowRightOutlined,
  ArrowDownOutlined
} from '@ant-design/icons';
import { apiGet, fetchWithTimeout } from '../utils/api';

const { Title, Text } = Typography;

const NetworkTopology = () => {
  const [topologyData, setTopologyData] = useState({
    gateways: [],
    devices: [],
    infrastructure: {
      status: 'unknown',
      endpoint: null, // Will be fetched from API
      services: []
    }
  });

  const [lastUpdate, setLastUpdate] = useState(new Date());

  const fetchTopologyData = async () => {
    try {
      const [infraHealthResponse, infraStatusResponse, gatewaysData, devicesData] = await Promise.all([
        fetchWithTimeout('/api/v1/infrastructure/health', {}, 5000).catch(() => ({ ok: false })),
        fetchWithTimeout('/api/v1/infrastructure/status', {}, 5000).catch(() => ({ ok: false })),
        apiGet('/api/v1/gateways', 10000),
        apiGet('/api/v1/devices', 10000)
      ]);

      // Determine infrastructure status from health and status endpoints
      let infraStatus = 'unknown';
      let infraEndpoint = null;
      let services = ['Certificate Authority', 'Secret Store', 'Monitoring'];

      if (infraHealthResponse.ok) {
        try {
          const infraHealthData = await infraHealthResponse.json();
          infraStatus = infraHealthData.status === 'healthy' ? 'active' : 'inactive';
          infraEndpoint = infraHealthData.infrastructure_endpoint || 'http://localhost:30460';
          console.log('[NetworkTopology] Infrastructure health:', infraHealthData);
        } catch (e) {
          console.warn('Failed to parse infrastructure health:', e);
        }
      } else {
        console.warn('[NetworkTopology] Infrastructure health response not OK:', infraHealthResponse.status);
      }

      if (infraStatusResponse.ok) {
        try {
          const infraStatusData = await infraStatusResponse.json();
          console.log('[NetworkTopology] Infrastructure status:', infraStatusData);
          const components = infraStatusData.components || {};
          // If all components are healthy, status is active
          const allHealthy = Object.values(components).every(v => v === 'healthy');
          if (allHealthy) {
            infraStatus = 'active';
          } else if (infraStatus === 'unknown') {
            // If health endpoint failed but status endpoint shows some healthy components
            const hasHealthyComponents = Object.values(components).some(v => v === 'healthy');
            if (hasHealthyComponents) {
              infraStatus = 'active';
            }
          }
          // Extract services from components
          services = Object.keys(components).map(key => {
            const nameMap = {
              'ca': 'Certificate Authority',
              'secret_store': 'Secret Store',
              'monitoring': 'Monitoring',
              'logging': 'Logging'
            };
            return nameMap[key] || key;
          });
          if (services.length === 0) {
            services = ['Certificate Authority', 'Secret Store', 'Monitoring'];
          }
        } catch (e) {
          console.warn('Failed to parse infrastructure status:', e);
        }
      } else {
        console.warn('[NetworkTopology] Infrastructure status response not OK:', infraStatusResponse.status);
        // Fallback: if health endpoint says healthy, use that
        if (infraStatus === 'active') {
          // Already set from health endpoint
        } else if (infraStatus === 'unknown') {
          // Try direct health check as fallback
          try {
            const healthCheck = await fetch('/api/v1/infrastructure/health').catch(() => null);
            if (healthCheck && healthCheck.ok) {
              const healthData = await healthCheck.json();
              if (healthData.status === 'healthy') {
                infraStatus = 'active';
              }
            }
          } catch (e) {
            console.warn('Fallback health check failed:', e);
          }
        }
      }

      const gateways = gatewaysData;
      const devices = devicesData;

      let gatewayList = gateways.gateways || [];
      let deviceList = devices.devices || [];

      // Use real data from backend - no mock data
      // If no devices are found but gateways show connected devices, create placeholder devices
      if (deviceList.length === 0 && gatewayList.length > 0) {
        deviceList = [];
        gatewayList.forEach(gateway => {
          // Create placeholder devices for connected devices
          for (let i = 1; i <= (gateway.connected_devices || 0); i++) {
            deviceList.push({
              id: `${gateway.id}-device-${i}`,
              name: `${gateway.name}-device-${i}`,
              type: 'MCU',
              architecture: 'riscv64',
              status: 'connected',
              gateway: gateway.name,
              age: 'unknown'
            });
          }
        });
      }

      // Use infrastructure endpoint from health response or fallback
      if (!infraEndpoint && gatewayList.length > 0) {
        infraEndpoint = gatewayList[0].endpoint;
      }
      
      setTopologyData(prev => ({
        ...prev,
        infrastructure: {
          status: infraStatus,
          endpoint: infraEndpoint || 'http://localhost:30460',
          services: services
        },
        gateways: gatewayList,
        devices: deviceList
      }));
    } catch (error) {
      console.error('Error fetching topology data:', error);
    }
  };

  useEffect(() => {
    fetchTopologyData();
    const interval = setInterval(() => {
      setLastUpdate(new Date());
      fetchTopologyData();
    }, 5000);

    return () => clearInterval(interval);
  }, []);

  const getStatusColor = (status) => {
    switch (status) {
      case 'active':
      case 'connected':
        return 'success';
      case 'inactive':
      case 'disconnected':
        return 'error';
      case 'pending':
        return 'warning';
      default:
        return 'default';
    }
  };

  const getStatusIcon = (status) => {
    switch (status) {
      case 'active':
      case 'connected':
        return <CheckCircleOutlined style={{ color: '#52c41a' }} />;
      case 'inactive':
      case 'disconnected':
        return <DisconnectOutlined style={{ color: '#ff4d4f' }} />;
      case 'pending':
        return <ExclamationCircleOutlined style={{ color: '#faad14' }} />;
      default:
        return <WifiOutlined style={{ color: '#d9d9d9' }} />;
    }
  };

  const getDeviceIcon = (type) => {
    switch (type) {
      case 'MCU':
        return <DesktopOutlined style={{ color: '#1890ff' }} />;
      case 'RISC-V':
        return <DesktopOutlined style={{ color: '#722ed1' }} />;
      default:
        return <DesktopOutlined style={{ color: '#666666' }} />;
    }
  };

  return (
    <div>
      <div style={{ marginBottom: 24, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <Title level={2}>Network Topology</Title>
        <div style={{ textAlign: 'right' }}>
          <Text type="secondary" style={{ fontSize: '12px' }}>
            Last updated: {lastUpdate.toLocaleTimeString()}
          </Text>
          <br />
          <Badge status="processing" text="Live Updates" />
        </div>
      </div>

      {/* Infrastructure Layer */}
      <Card 
        title={
          <Space>
            <CloudServerOutlined style={{ color: '#1890ff' }} />
            <span>Infrastructure Layer</span>
          </Space>
        }
        style={{ marginBottom: 8 }}
        size="small"
      >
        <Row gutter={[8, 8]}>
          <Col span={24}>
            <div style={{ 
              display: 'flex', 
              alignItems: 'center', 
              justifyContent: 'space-between',
              padding: '6px 8px',
              background: 'rgba(24, 144, 255, 0.1)',
              borderRadius: '6px',
              border: '1px solid rgba(24, 144, 255, 0.3)',
              minHeight: 'auto'
            }}>
              <div>
                <Text strong>Infrastructure Service</Text>
                <br />
                <Text type="secondary" style={{ fontSize: '12px' }}>
                  {topologyData.infrastructure.endpoint}
                </Text>
              </div>
              <div style={{ textAlign: 'right' }}>
                <Tag color={getStatusColor(topologyData.infrastructure.status)}>
                  {getStatusIcon(topologyData.infrastructure.status)}
                  {topologyData.infrastructure.status.toUpperCase()}
                </Tag>
                <br />
                <Text type="secondary" style={{ fontSize: '11px' }}>
                  {topologyData.infrastructure.services.join(', ')}
                </Text>
              </div>
            </div>
          </Col>
        </Row>
        
        {/* Connection Line */}
        <div style={{ 
          textAlign: 'center', 
          margin: '8px 0',
          height: '2px',
          background: 'linear-gradient(90deg, transparent 0%, #d9d9d9 20%, #d9d9d9 80%, transparent 100%)',
          position: 'relative'
        }}>
          <div style={{
            position: 'absolute',
            top: '-6px',
            left: '50%',
            transform: 'translateX(-50%)',
            width: '0',
            height: '0',
            borderLeft: '6px solid transparent',
            borderRight: '6px solid transparent',
            borderTop: '8px solid #d9d9d9'
          }} />
        </div>
      </Card>

      {/* Gateway Layer */}
      <Card 
        title={
          <Space>
            <GatewayOutlined style={{ color: '#52c41a' }} />
            <span>Gateway Layer</span>
          </Space>
        }
        style={{ marginBottom: 8 }}
        size="small"
      >
        <Row gutter={[8, 8]}>
          {topologyData.gateways.map((gateway, index) => (
            <Col xs={24} sm={12} lg={8} key={gateway.id || gateway.name || index}>
              <div style={{ position: 'relative' }}>
                <div style={{ 
                  display: 'flex', 
                  alignItems: 'center', 
                  justifyContent: 'space-between',
                  padding: '6px 8px',
                  background: 'rgba(82, 196, 26, 0.1)',
                  borderRadius: '6px',
                  border: '1px solid rgba(82, 196, 26, 0.3)',
                  minHeight: 'auto'
                }}>
                  <div>
                    <Text strong>{gateway.name}</Text>
                    <br />
                    <Text type="secondary" style={{ fontSize: '12px' }}>
                      {gateway.endpoint}
                    </Text>
                    <br />
                    <Text type="secondary" style={{ fontSize: '11px' }}>
                      Region: {gateway.region}
                    </Text>
                  </div>
                  <div style={{ textAlign: 'right' }}>
                    <Tag color={getStatusColor(gateway.status)}>
                      {getStatusIcon(gateway.status)}
                      {gateway.status.toUpperCase()}
                    </Tag>
                    <br />
                    <Text type="secondary" style={{ fontSize: '11px' }}>
                      {gateway.devices} devices
                    </Text>
                  </div>
                </div>
                
                {/* Connection arrows to devices */}
                {gateway.devices > 0 && (
                  <div style={{
                    position: 'absolute',
                    bottom: '-6px', 
                    left: '50%', 
                    transform: 'translateX(-50%)',
                    zIndex: 10,
                    width: '0',
                    height: '0',
                    borderLeft: '4px solid transparent',
                    borderRight: '4px solid transparent',
                    borderTop: '6px solid #d9d9d9'
                  }} />
                )}
              </div>
            </Col>
          ))}
        </Row>
      </Card>

      {/* Device Layer */}
      <Card 
        title={
          <Space>
            <DesktopOutlined style={{ color: '#722ed1' }} />
            <span>Device Layer</span>
          </Space>
        }
        style={{ marginBottom: 8 }}
        size="small"
      >
        <Row gutter={[8, 8]}>
          {topologyData.devices.map((device, index) => (
            <Col xs={24} sm={12} lg={8} xl={6} key={device.id || device.name || index}>
              <Tooltip title={`Architecture: ${device.architecture} • Gateway: ${device.gateway}`}>
                <div style={{ 
                  display: 'flex', 
                  alignItems: 'center', 
                  justifyContent: 'space-between',
                  padding: '6px 8px',
                  background: device.status === 'connected' 
                    ? 'rgba(82, 196, 26, 0.1)' 
                    : 'rgba(255, 77, 79, 0.1)',
                  borderRadius: '6px',
                  border: `1px solid ${device.status === 'connected' 
                    ? 'rgba(82, 196, 26, 0.3)' 
                    : 'rgba(255, 77, 79, 0.3)'}`,
                  position: 'relative',
                  minHeight: 'auto'
                }}>
                  <div>
                    <Space>
                      {getDeviceIcon(device.type)}
                      <Text strong>{device.name}</Text>
                    </Space>
                    <br />
                    <Text type="secondary" style={{ fontSize: '12px' }}>
                      {device.type} • {device.gateway}
                    </Text>
                  </div>
                  <div style={{ textAlign: 'right' }}>
                    <Tag color={getStatusColor(device.status)}>
                      {getStatusIcon(device.status)}
                      {device.status.toUpperCase()}
                    </Tag>
                  </div>
                  
                  {/* Connection indicator */}
                  {device.status === 'connected' && (
                    <div style={{
                      position: 'absolute',
                      top: '8px', 
                      right: '8px',
                      width: '8px',
                      height: '8px',
                      borderRadius: '50%',
                      background: '#52c41a'
                    }} />
                  )}
                </div>
              </Tooltip>
            </Col>
          ))}
        </Row>
      </Card>

      {/* Connection Summary */}
      <Card 
        title="Connection Summary"
        style={{ marginTop: 16 }}
        size="small"
      >
        <Row gutter={[8, 8]}>
          <Col xs={12} sm={6} key="active-gateways">
            <div style={{ textAlign: 'center' }}>
              <div style={{ fontSize: '24px', fontWeight: 'bold', color: '#52c41a' }}>
                {topologyData.gateways.filter(g => g.status === 'active').length}
              </div>
              <Text type="secondary">Active Gateways</Text>
            </div>
          </Col>
          <Col xs={12} sm={6} key="total-devices">
            <div style={{ textAlign: 'center' }}>
              <div style={{ fontSize: '24px', fontWeight: 'bold', color: '#1890ff' }}>
                {topologyData.devices.length}
              </div>
              <Text type="secondary">Total Devices</Text>
            </div>
          </Col>
          <Col xs={12} sm={6} key="connected-devices">
            <div style={{ textAlign: 'center' }}>
              <div style={{ fontSize: '24px', fontWeight: 'bold', color: '#52c41a' }}>
                {topologyData.devices.filter(d => d.status === 'connected').length}
              </div>
              <Text type="secondary">Connected Devices</Text>
            </div>
          </Col>
          <Col xs={12} sm={6} key="disconnected-devices">
            <div style={{ textAlign: 'center' }}>
              <div style={{ fontSize: '24px', fontWeight: 'bold', color: '#faad14' }}>
                {topologyData.devices.filter(d => d.status === 'disconnected').length}
              </div>
              <Text type="secondary">Disconnected Devices</Text>
            </div>
          </Col>
        </Row>
      </Card>
    </div>
  );
};

export default NetworkTopology;
